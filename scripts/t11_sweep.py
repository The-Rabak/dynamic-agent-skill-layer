#!/usr/bin/env python3
"""T11 hybrid-vs-dense re-sweep orchestrator — drives the REAL mcp-server over HTTP.

WHY this orchestrator exists
----------------------------
The prior T04 sweep returned MRR = 0.767 across ALL arms because:
  1. The ranking brain (eq.3 dense cosine) is shared; only candidate-generation
     can differ between backends at 262-skill corpus scale.
  2. MRR@3 is saturated (quantized to {1, 0.5, 0.33, 0}).

This sweep surfaces the signals that actually distinguish backends:
  - candidate_recall@50  — the only metric candidate-gen can move
  - paired per-query rank diffs + sign test — disambiguates ties statistically
  - α=0 crater check     — fixture discriminability proof (if α=0 doesn't
                            crater MRR, the fixture is broken, not the backend)

All measurement drives the REAL running mcp-server over HTTP (no in-process
reconstruction); see memory rule "measurement-drives-real-app-no-in-process-
reconstruction".

Readiness gate
--------------
This orchestrator polls GET http://127.0.0.1:3001/health until HTTP 200 to
confirm the server is ready.  200 = snapshot loaded and serving; 503 = still
warming (qwen3 re-embed in progress).  This replaces the old warmup-query
approach: T17 made /health honest, eliminating the warming-while-healthy
measurement-corruption window.

The stuck deadline (default 2700s) is a STUCK DETECTOR, NOT a work cap.  A
healthy qwen3 boot on a GPU-only system can take many minutes.  The deadline
fires ONLY when the health endpoint stays 503 for longer than the deadline,
which indicates graph-builder has stalled.  Do NOT lower this to cap legitimate
work (project memory: no-arbitrary-limits).

Usage
-----
  python3 scripts/t11_sweep.py --run-id <timestamp> [options]
  python3 scripts/t11_sweep.py --help
"""
import argparse
import json
import os
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

# Reuse helpers from the sibling sweep module instead of duplicating them.
# This keeps the reboot lifecycle, Qdrant-collection logic, and pg-purge
# helpers DRY.  The ONE behavioral change is the readiness signal: we poll
# /health for 200 instead of the warmup-query approach.
#
# The sys.path insert is necessary because scripts/ is not a package.
# This mirrors the import pattern used by other orchestrators in this repo.
_SCRIPTS_DIR = Path(__file__).parent.resolve()
if str(_SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(_SCRIPTS_DIR))

import retrieval_quality_live as _live  # noqa: E402 — after sys.path insert
import retrieval_quality_sweep as _sweep  # noqa: E402 — after sys.path insert

# Also pull metric functions from the T11 metrics module.
import t11_metrics as _metrics  # noqa: E402

MCP_URL = "http://127.0.0.1:3001/mcp"
HEALTH_URL = "http://127.0.0.1:3001/health"
JUDGE_MODEL = "claude-sonnet-4-6"
FIXTURE_DEFAULT = "tests/fixtures/retrieval_quality_262_corpus_labeled.json"
REPORT_DIR_DEFAULT = Path("tests/e2e/reports/t11")

# ── Arm configuration table ───────────────────────────────────────────────────
# Each entry: (label, env-override dict).
# Empty dict = server defaults (snapshot_dense, qwen3-embedding:4b, alpha=0.45).
# These mirror the arms described in the T11 ticket.
# Every arm pins OLLAMA_EMBED_MODEL=qwen3-embedding:4b.  Rationale: the sibling
# sweep helpers (reboot_arm / _target_collection_for_arm / assert_collection_nonempty)
# derive the Qdrant collection name from os.environ["OLLAMA_EMBED_MODEL"] and
# default it to "nomic-embed-text" when absent.  The live T10 corpus is qwen3, so
# without this pin the qdrant_hybrid rebuild-poll watches the nonexistent
# skills__nomic-embed-text__hybrid collection (404 forever) while graph-builder
# actually populates skills__qwen3-embedding-4b__hybrid.  set_env() clears all arm
# env between arms, so the model must live in each arm's override dict (a shell-level
# export would be wiped by set_env).
_QWEN = {"OLLAMA_EMBED_MODEL": "qwen3-embedding:4b"}
CONFIGS: list[tuple[str, dict]] = [
    ("snapshot_dense",  {**_QWEN}),
    ("snapshot_hybrid", {**_QWEN, "RETRIEVAL_BACKEND": "snapshot_hybrid"}),
    ("qdrant_hybrid",   {**_QWEN, "RETRIEVAL_BACKEND": "qdrant_hybrid"}),
    ("dense_views_on",  {**_QWEN, "RETRIEVAL_DENSE_VIEWS": "true"}),
    # α=0 control: removes dense cosine signal entirely.  A healthy fixture
    # must crater MRR by ≥50% relative vs snapshot_dense.  If it does not,
    # the fixture cannot discriminate and the sweep verdict is void.
    ("alpha0_control",  {**_QWEN, "RETRIEVAL_ALPHA": "0.0"}),
]


# ── Health-based readiness gate (T17 honesty) ─────────────────────────────────

def _poll_health_until_ready(
    health_url: str = HEALTH_URL,
    poll_interval_s: float = 5.0,
    stuck_deadline_s: float = 2700.0,
) -> None:
    """Block until the mcp-server /health endpoint returns HTTP 200.

    T17 made /health honest: 200 = snapshot loaded and serving; 503 = still
    warming (qwen3 re-embed).  This replaces the old warmup-query approach
    used in retrieval_quality_sweep.py's wait_ready() which queried find_skill
    — that created a window where the server would answer find_skill queries
    with a partially-warmed snapshot, corrupting early-arm measurements.

    The stuck_deadline_s is a STUCK DETECTOR, NOT a work cap.  qwen3 on a
    local GPU can take many minutes to re-embed a 262-skill corpus; the
    deadline fires only when the server stays non-200 for the full window,
    indicating a real stall (Ollama unreachable, graph-builder crashed, etc.).
    Do NOT lower this value to cap legitimate work.

    Args:
        health_url:       URL to poll (default http://127.0.0.1:3001/health).
        poll_interval_s:  seconds between probes.
        stuck_deadline_s: raise RuntimeError only when this elapses with no 200.

    Raises:
        RuntimeError: if stuck_deadline_s elapses without a 200 response.
    """
    start = time.monotonic()
    while True:
        elapsed = time.monotonic() - start
        try:
            with urllib.request.urlopen(health_url, timeout=10) as resp:
                if resp.status == 200:
                    body = resp.read().decode("utf-8", errors="replace")
                    print(
                        f"  readiness: /health 200 after {elapsed:.0f}s — server ready",
                        flush=True,
                    )
                    return
                status = resp.status
        except urllib.error.HTTPError as exc:
            status = exc.code
        except Exception as exc:
            status = f"error:{exc}"

        print(
            f"  readiness: /health returned {status} at {elapsed:.0f}s; "
            f"server still warming — polling ...",
            flush=True,
        )

        if elapsed >= stuck_deadline_s:
            raise RuntimeError(
                f"STUCK: mcp-server /health did not return 200 within {stuck_deadline_s:.0f}s.\n"
                f"Last status: {status}\n"
                f"This is a real stuck state (Ollama unreachable, graph-builder crashed, etc.) —\n"
                f"the deadline is a stuck-detector, NOT a work cap.  Check mcp-server logs.\n"
                f"Do NOT lower the deadline to cap legitimate qwen3 re-embed work."
            )

        time.sleep(poll_interval_s)


def _health_based_wait_ready(deadline_s: int = 600) -> None:
    """Drop-in replacement for retrieval_quality_sweep.wait_ready().

    The sibling sweep's wait_ready() uses a HARDCODED warmup find_skill query
    ("conventional commits with co-authored-by trailer") as its readiness
    signal.  That prompt was a known topic in the OLD 234-skill corpus but
    matches NOTHING in the T10 262-skill corpus, so warmup_query() returns an
    empty list and wait_ready() times out at 600s even though the server is
    fully up.  reboot_arm() calls wait_ready() internally, so every
    backend-changing arm (snapshot_hybrid, qdrant_hybrid) inherits that hang.

    We monkeypatch _sweep.wait_ready with this corpus-agnostic /health-200 gate
    (T17 honesty) so reboot_arm's internal readiness wait no longer depends on a
    stale corpus-specific probe query.  The deadline maps onto the stuck-detector
    in _poll_health_until_ready (treated as a stuck detector, not a work cap).
    """
    _poll_health_until_ready(stuck_deadline_s=max(float(deadline_s), 2700.0))


# Corpus-agnostic readiness for reboot_arm's INTERNAL wait (see docstring above).
_sweep.wait_ready = _health_based_wait_ready


# ── find_skill with score capture ─────────────────────────────────────────────

def _find_skill_with_scores(prompt: str, limit: int) -> list[dict]:
    """Call the live server's find_skill tool; return dicts with name + score.

    Each returned dict has:
        name  — skill name string
        score — float parsed from the eq.3 semantic relevance field (e.g. 0.836)
        fusion_rank_score — float (raw fusion rank score from the match)

    Raises RuntimeError on any RPC error or missing/None score field (no silent
    fallback — machine no-fakes rule: a None score must raise, not default to 0).

    Args:
        prompt: the query text to send to find_skill.
        limit:  number of results to request.
    """
    body = json.dumps({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {
            "name": "find_skill",
            "arguments": {"prompt": prompt, "limit": limit},
        },
    }).encode()
    req = urllib.request.Request(
        MCP_URL, data=body, headers={"Content-Type": "application/json"}
    )
    with urllib.request.urlopen(req, timeout=60) as resp:
        r = json.loads(resp.read())

    if "error" in r and r["error"]:
        raise RuntimeError(
            f"find_skill RPC error for prompt {prompt!r}: {r['error']}"
        )

    matches = r.get("result", {}).get("matches", [])
    results = []
    for m in matches:
        name = m.get("name")
        if not name:
            raise RuntimeError(
                f"find_skill match missing 'name' field for prompt {prompt!r}: {m!r}"
            )
        raw_score = m.get("score")
        if raw_score is None:
            raise RuntimeError(
                f"find_skill match '{name}' has None 'score' for prompt {prompt!r}. "
                f"T06 must expose per-match score; this is a hard precondition."
            )
        try:
            score = float(raw_score)
        except (TypeError, ValueError) as exc:
            raise RuntimeError(
                f"find_skill match '{name}' has unparseable score {raw_score!r}: {exc}"
            ) from exc

        raw_fusion = m.get("fusion_rank_score")
        fusion = float(raw_fusion) if raw_fusion is not None else None

        results.append({"name": name, "score": score, "fusion_rank_score": fusion})

    return results


# ── Judge (same invocation as retrieval_quality_live.py) ─────────────────────

def _judge_query(query_text: str, candidates: list[dict]) -> dict[str, bool]:
    """Batch-judge candidates with the REAL claude CLI.

    Delegates to retrieval_quality_live.judge_query to keep relevance judgements
    consistent across T04 and T11 — no duplicated subprocess invocation.

    Args:
        query_text: the user query string.
        candidates: list of {name, description} dicts.
    """
    return _live.judge_query(query_text, candidates)


# ── Per-arm metric computation ────────────────────────────────────────────────

def _first_relevant_rank(ranked_names: list[str], rel: set[str]) -> int:
    """Return the 1-indexed rank of the first relevant result, or 999 (not found).

    Rank 999 is the sentinel used in paired_rank_diffs for a query where no
    relevant result appeared in the candidate list.
    """
    for i, name in enumerate(ranked_names):
        if name in rel:
            return i + 1
    return 999


def _compute_arm_metrics(
    positives: list[dict],
    negatives: list[dict],
    mrr_limit: int,
    candidate_limit: int,
    use_judge: bool,
    verdict_cache: dict,
) -> dict:
    """Drive find_skill for all queries and compute the full T11 metric set.

    Queries the REAL mcp-server twice per positive query:
      1. At mrr_limit (for ranking metrics: MRR@3, MRR@10, nDCG@3, hit@3, recall@3)
      2. At candidate_limit (for candidate_recall@limit)

    Args:
        positives:       positive queries (kind != "negative").
        negatives:       negative queries (kind == "negative").
        mrr_limit:       find_skill depth for ranking metrics (default 10).
        candidate_limit: find_skill depth for candidate-recall (default 50).
        use_judge:       whether to augment relevance with the LLM judge.
        verdict_cache:   shared {(query_id, skill_name): bool} cache.

    Returns a dict with all per-arm aggregate metrics, per-query vectors, and
    the score histogram over top-1 scores of positive queries.
    """
    per_query_rows = []
    first_relevant_ranks_mrr = []
    top1_scores = []

    for q in positives:
        qid = q["id"]
        rel_set = {q["anchor"]} | set(q.get("relevant", []))

        # Query at mrr_limit for ranking metrics.
        mrr_matches = _find_skill_with_scores(q["text"], mrr_limit)
        mrr_names = [m["name"] for m in mrr_matches]

        # Query at candidate_limit for candidate-recall.
        cand_matches = _find_skill_with_scores(q["text"], candidate_limit)
        cand_names = [m["name"] for m in cand_matches]

        # Optionally augment rel_set via the judge.
        if use_judge:
            to_judge = [
                {"name": m["name"], "description": ""}
                for m in mrr_matches
                if (qid, m["name"]) not in verdict_cache
            ]
            if to_judge:
                verdicts = _judge_query(q["text"], to_judge)
                for name in [c["name"] for c in to_judge]:
                    verdict_cache[(qid, name)] = bool(verdicts.get(name, False))
            for name in mrr_names:
                if verdict_cache.get((qid, name), False):
                    rel_set.add(name)

        # Ranking metrics (over mrr window).
        rr_rank = _first_relevant_rank(mrr_names, rel_set)
        first_relevant_ranks_mrr.append(rr_rank)

        # Top-1 score for histogram.
        if mrr_matches:
            top1_scores.append(mrr_matches[0]["score"])

        cand_recall = _metrics.candidate_recall_at_limit(cand_names, rel_set)

        row = {
            "id": qid,
            "kind": q["kind"],
            "split": q["split"],
            "anchor": q.get("anchor"),
            "first_relevant_rank": rr_rank,
            "rr": _metrics.reciprocal_rank(mrr_names[:3], rel_set),
            "rr_at10": _metrics.reciprocal_rank(mrr_names, rel_set),
            "ndcg_at3": _metrics.ndcg_at_k(mrr_names, rel_set, 3),
            "hit_at3": _metrics.hit_at_k(mrr_names, rel_set, 3),
            "recall_at3": _metrics.recall_at_k(mrr_names, rel_set, 3),
            "precision_at1": _metrics.precision_at_1(mrr_names, rel_set),
            "candidate_recall": cand_recall,
            "top3_names": mrr_names[:3],
            "relevant_set": sorted(rel_set),
        }
        per_query_rows.append(row)

    n = len(per_query_rows)

    def _mean(key: str) -> float:
        return sum(r[key] for r in per_query_rows) / n if n else 0.0

    # MRR values are computed directly from per_query_rows (no re-querying):
    # "rr"    = reciprocal rank capped at the top-3 window (= MRR@3 contribution)
    # "rr_at10" = reciprocal rank over the full mrr_limit window (= MRR@10 contribution)
    mrr_at3_from_rows = _mean("rr")
    mrr_at10_from_rows = _mean("rr_at10")

    # Negatives: non-empty result for off-topic query = fabricated match.
    fabricated = 0
    neg_rows = []
    for q in negatives:
        matches = _find_skill_with_scores(q["text"], mrr_limit)
        is_fabricated = len(matches) > 0
        fabricated += 1 if is_fabricated else 0
        neg_rows.append({
            "id": q["id"],
            "served": [m["name"] for m in matches[:3]],
            "fabricated": is_fabricated,
        })
    no_match_precision = 1.0 - (fabricated / len(negatives)) if negatives else None

    score_histogram = _metrics.histogram(top1_scores) if top1_scores else None

    return {
        "n_positives": n,
        "n_negatives": len(negatives),
        "mrr_at3": round(mrr_at3_from_rows, 6),
        "mrr_at10": round(mrr_at10_from_rows, 6),
        "ndcg_at3": round(_mean("ndcg_at3"), 6),
        "hit_at3": round(_mean("hit_at3"), 6),
        "recall_at3": round(_mean("recall_at3"), 6),
        "precision_at1": round(_mean("precision_at1"), 6),
        "candidate_recall_at_limit": round(_mean("candidate_recall"), 6),
        "no_match_precision": no_match_precision,
        "first_relevant_ranks": first_relevant_ranks_mrr,
        "per_query": per_query_rows,
        "neg_detail": neg_rows,
        "top1_score_histogram": score_histogram,
    }


# ── Main orchestrator ─────────────────────────────────────────────────────────

def main() -> None:
    """Parse args, run selected arms against the live server, emit JSON report."""
    ap = argparse.ArgumentParser(
        description=(
            "T11 hybrid-vs-dense re-sweep orchestrator.  Drives the REAL mcp-server "
            "over HTTP for each arm, computes MRR@3/10, nDCG@3, candidate-recall@50, "
            "paired rank diffs + sign test, and α=0 crater check.  Requires the server "
            "to be running at http://127.0.0.1:3001.  Use --run-id to tag the output file."
        )
    )
    ap.add_argument(
        "--run-id",
        required=True,
        help=(
            "Timestamp or label for this sweep run (e.g. '2026-06-12T14-00').  "
            "Used in the output filename: tests/e2e/reports/t11/sweep_<run-id>.json.  "
            "Do NOT use datetime.now() — accept this arg for reproducibility."
        ),
    )
    ap.add_argument(
        "--fixture",
        default=FIXTURE_DEFAULT,
        help=f"Path to the labeled corpus JSON (default: {FIXTURE_DEFAULT}).",
    )
    ap.add_argument(
        "--split",
        choices=["tuning", "held_out", "all"],
        default="all",
        help="Corpus split to measure on (default: all).",
    )
    ap.add_argument(
        "--arms",
        default="",
        help=(
            "Comma-separated list of arm labels to run (e.g. 'snapshot_dense,qdrant_hybrid'). "
            "Omit or leave empty to run all arms in CONFIGS."
        ),
    )
    ap.add_argument(
        "--limit",
        type=int,
        default=10,
        help="find_skill depth for ranking metrics (MRR@10 window; default 10).",
    )
    ap.add_argument(
        "--candidate-limit",
        type=int,
        default=50,
        help="find_skill depth for candidate-recall (default 50).",
    )
    ap.add_argument(
        "--judge",
        action="store_true",
        default=False,
        help=(
            "Augment relevance via the real claude CLI judge (OFF by default "
            "for fast matrix runs; turn on for headline arms)."
        ),
    )
    ap.add_argument(
        "--health-stuck-deadline",
        type=float,
        default=2700.0,
        help=(
            "Seconds before the /health readiness poll raises a STUCK error "
            "(default 2700 = 45 min).  This is a stuck-detector, NOT a work cap.  "
            "qwen3 re-embed on a slow GPU can take many minutes; do NOT lower "
            "this to cap legitimate work."
        ),
    )
    ap.add_argument(
        "--out-dir",
        default=str(REPORT_DIR_DEFAULT),
        help=f"Output directory for the sweep JSON report (default: {REPORT_DIR_DEFAULT}).",
    )
    args = ap.parse_args()

    # Resolve which arms to run.
    arm_filter = {a.strip() for a in args.arms.split(",") if a.strip()}
    selected_configs = [
        (label, overrides)
        for label, overrides in CONFIGS
        if not arm_filter or label in arm_filter
    ]
    if not selected_configs:
        print(
            f"ERROR: no arms match --arms={args.arms!r}. "
            f"Available: {[l for l, _ in CONFIGS]}",
            file=sys.stderr,
        )
        sys.exit(1)

    # Load fixture.
    fixture_path = Path(args.fixture)
    if not fixture_path.exists():
        print(
            f"ERROR: fixture not found: {fixture_path}\n"
            f"Expected schema: {{\"queries\": [{{id, kind, split, text, anchor, relevant}}]}}",
            file=sys.stderr,
        )
        sys.exit(1)
    fixture = json.loads(fixture_path.read_text())
    all_queries = fixture["queries"]

    if args.split != "all":
        all_queries = [q for q in all_queries if q.get("split") == args.split]

    positives = [q for q in all_queries if q.get("kind") != "negative"]
    negatives = [q for q in all_queries if q.get("kind") == "negative"]

    print(
        f"\n=== T11 SWEEP  run-id={args.run_id}  split={args.split}  "
        f"arms={[l for l, _ in selected_configs]} ===",
        flush=True,
    )
    print(
        f"fixture: {fixture_path}  "
        f"positives={len(positives)}  negatives={len(negatives)}",
        flush=True,
    )

    verdict_cache: dict = {}
    arm_results: dict[str, dict] = {}

    for label, overrides in selected_configs:
        print(f"\n########## ARM: {label}  {overrides or '(defaults)'} ##########", flush=True)

        # Apply env overrides and reboot the arm.
        _sweep.set_env(overrides)

        # Determine reboot strategy.  ONLY backend-changing arms
        # (snapshot_hybrid, qdrant_hybrid) need reboot_arm — they require
        # graph-builder to (re)write the hybrid/sparse Qdrant collection before
        # mcp-server reads it.  alpha0_control (RETRIEVAL_ALPHA) and
        # dense_views_on (RETRIEVAL_DENSE_VIEWS) are mcp-server-only knobs
        # (ranking weight / boot-time fusion flag, both served from the T17
        # embedding cache) and need only an mcp-server restart.
        #
        # Critical: reboot_arm internally calls the sibling sweep's
        # wait_ready(), which uses a warmup *find_skill query* as its readiness
        # signal.  Under alpha0_control that query returns NO matches (the
        # semantic term is zeroed, so every candidate falls below the relevance
        # floor), so wait_ready() never observes a non-empty result and times
        # out at 600s even though the server is fully up.  Routing alpha0 (and
        # the other mcp-only knob) through reboot_mcp avoids that internal
        # warmup-query wait entirely; the honest /health-200 poll below is the
        # real readiness gate (T17).
        if _sweep._is_arm_config(label) or label in ("snapshot_hybrid", "qdrant_hybrid"):
            print(f"  reboot_arm ({label}) ...", flush=True)
            _sweep.reboot_arm(overrides)
        else:
            print(f"  reboot_mcp ({label}) ...", flush=True)
            _sweep.reboot_mcp()

        # T17 readiness gate: poll /health for 200 instead of warmup query.
        # 200 = snapshot loaded; 503 = still warming (qwen3 re-embed in flight).
        print(f"  polling /health for readiness (stuck-deadline={args.health_stuck_deadline:.0f}s) ...", flush=True)
        _poll_health_until_ready(
            health_url=HEALTH_URL,
            stuck_deadline_s=args.health_stuck_deadline,
        )

        # Measure this arm.
        arm_metrics = _compute_arm_metrics(
            positives=positives,
            negatives=negatives,
            mrr_limit=args.limit,
            candidate_limit=args.candidate_limit,
            use_judge=args.judge,
            verdict_cache=verdict_cache,
        )
        arm_results[label] = {
            "label": label,
            "env_overrides": overrides,
            "metrics": arm_metrics,
        }

        m = arm_metrics
        print(
            f"  MRR@3={m['mrr_at3']:.3f}  MRR@10={m['mrr_at10']:.3f}  "
            f"nDCG@3={m['ndcg_at3']:.3f}  hit@3={m['hit_at3']:.3f}  "
            f"recall@3={m['recall_at3']:.3f}  cand_recall@{args.candidate_limit}={m['candidate_recall_at_limit']:.3f}  "
            f"no_match_prec={m['no_match_precision']}",
            flush=True,
        )

    # ── Paired comparisons vs snapshot_dense baseline ─────────────────────────
    paired_comparisons: dict[str, dict] = {}
    baseline_label = "snapshot_dense"
    baseline = arm_results.get(baseline_label)

    if baseline:
        baseline_ranks = baseline["metrics"]["first_relevant_ranks"]
        baseline_mrr = baseline["metrics"]["mrr_at3"]

        for label, result in arm_results.items():
            if label == baseline_label:
                continue
            candidate_ranks = result["metrics"]["first_relevant_ranks"]

            if len(baseline_ranks) != len(candidate_ranks):
                print(
                    f"  WARNING: rank vector length mismatch for {label} vs {baseline_label} "
                    f"({len(candidate_ranks)} vs {len(baseline_ranks)}); skipping paired comparison.",
                    flush=True,
                )
                continue

            # A = candidate, B = baseline, so the result reads with the printed
            # "<candidate> vs <baseline>" label: n_a_better = queries the
            # candidate arm improved, mean_rank_delta < 0 = candidate better.
            # (sign_test is two-sided/symmetric, so p is unaffected by the order.)
            diffs = _metrics.paired_rank_diffs(candidate_ranks, baseline_ranks)
            p_value = _metrics.sign_test(diffs["n_a_better"], diffs["n_b_better"])

            # α=0 control: check if it craters the baseline.
            crater = None
            if label == "alpha0_control":
                crater = _metrics.crater_check(
                    baseline_mrr, result["metrics"]["mrr_at3"]
                )
                if not crater["craters"]:
                    print(
                        f"\n  WARNING: alpha0_control DID NOT crater MRR "
                        f"(rel_drop={crater['rel_drop']:.3f} < 0.50).  "
                        f"The fixture may not discriminate — treat the sweep verdict with caution.",
                        flush=True,
                    )
                else:
                    print(
                        f"\n  OK: alpha0_control craters MRR by {crater['rel_drop']:.1%} "
                        f"(rel_drop={crater['rel_drop']:.3f} >= 0.50) — fixture discriminates.",
                        flush=True,
                    )

            paired_comparisons[label] = {
                "vs_baseline": baseline_label,
                "rank_diffs": diffs,
                "sign_test_p_value": p_value,
                "crater_check": crater,
                "verdict": (
                    "significant" if p_value < 0.05
                    else "not-significant"
                ),
            }
            print(
                f"  paired {label} vs {baseline_label}: "
                f"n_a_better={diffs['n_a_better']} n_b_better={diffs['n_b_better']} "
                f"n_tie={diffs['n_tie']} mean_delta={diffs['mean_rank_delta']:.2f} "
                f"p={p_value:.4f} ({paired_comparisons[label]['verdict']})",
                flush=True,
            )
    else:
        print(
            f"\n  NOTE: '{baseline_label}' was not in the selected arms; "
            f"skipping paired comparisons.",
            flush=True,
        )

    # ── Emit report ───────────────────────────────────────────────────────────
    report = {
        "run_id": args.run_id,
        "fixture": str(fixture_path),
        "split": args.split,
        "mrr_limit": args.limit,
        "candidate_limit": args.candidate_limit,
        "judge_enabled": args.judge,
        "n_positives": len(positives),
        "n_negatives": len(negatives),
        "arms": arm_results,
        "paired_comparisons": paired_comparisons,
    }

    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    out_path = out_dir / f"sweep_{args.run_id}.json"
    out_path.write_text(json.dumps(report, indent=1))

    print(f"\n=== SWEEP COMPLETE ===")
    print(f"report: {out_path}")


if __name__ == "__main__":
    main()
