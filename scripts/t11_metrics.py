#!/usr/bin/env python3
"""T11 measurement instruments — pure metric functions for the hybrid-vs-dense re-sweep.

WHY this module exists
----------------------
The prior sweep (T04) returned identical MRR@3 = 0.767 across ALL backends
(snapshot_dense / snapshot_hybrid / qdrant_hybrid).  The root cause:

  1. At 262-skill corpus scale, the ranking brain (eq.3 dense cosine) is the
     same for every arm.  Only *candidate generation* differs between backends,
     so only *candidate-recall* can move — not top-3 ranking.
  2. MRR@3 is quantized to {1, 0.5, 0.33, 0} and saturated (no signal left).

The instruments required to produce a real verdict are therefore:
  - candidate_recall_at_limit  — the ONLY metric candidate-gen can move
  - paired per-query rank diffs + sign test — mean equality at 3 decimals is
    NOT a verdict; we need a per-query paired comparison
  - α=0 crater check — proves the fixture can discriminate at all; if α=0
    does NOT crater MRR the fixture is broken, not the backend

All functions are pure (no I/O, no server calls, no state).  The live sweep
orchestrator (t11_sweep.py) calls these after collecting server responses.

Formulas match scripts/retrieval_quality_live.py exactly so T11 numbers are
comparable to the existing quality history.
"""
import argparse
import math
import sys
from typing import Sequence


# ── Core IR metrics (formulas match retrieval_quality_live.py) ──────────────

def reciprocal_rank(ranked: Sequence[str], rel: set[str]) -> float:
    """Return the reciprocal of the first-relevant rank (1-indexed), or 0.0.

    The rank is unbounded; callers that want a bounded @k version should use
    ``mrr_at_k`` instead, which returns 0.0 when the first relevant item
    appears beyond position k.

    Args:
        ranked: ordered list of candidate skill names (position 0 = rank 1).
        rel:    set of skill names considered relevant for this query.
    """
    for i, name in enumerate(ranked):
        if name in rel:
            return 1.0 / (i + 1)
    return 0.0


def mrr_at_k(queries_ranked: Sequence[Sequence[str]],
             queries_rel: Sequence[set[str]],
             k: int) -> float:
    """Mean Reciprocal Rank at k, averaged over a list of queries.

    For each query, only the first relevant item within the top-k window counts;
    if the first relevant item is beyond position k, it contributes 0.0.  This
    is the standard MRR@k definition used in the T11 sweep (MRR@3 and MRR@10
    are the two product arms).

    Args:
        queries_ranked: per-query ordered result lists.
        queries_rel:    per-query relevant-set (must be same length as ranked).
        k:              window size; items beyond rank k are ignored.
    """
    if not queries_ranked:
        return 0.0
    total = 0.0
    for ranked, rel in zip(queries_ranked, queries_rel):
        truncated = list(ranked)[:k]
        total += reciprocal_rank(truncated, rel)
    return total / len(queries_ranked)


def ndcg_at_k(ranked: Sequence[str], rel: set[str], k: int) -> float:
    """Normalised Discounted Cumulative Gain at k.

    Binary relevance: 1 if the item is in rel, else 0.  Matches the formula in
    retrieval_quality_live.py so historic numbers are directly comparable.

    Args:
        ranked: ordered result list.
        rel:    set of relevant skill names.
        k:      ranking window.
    """
    if not rel:
        return 0.0
    dcg = sum(
        (1.0 if name in rel else 0.0) / math.log2(i + 2)
        for i, name in enumerate(ranked[:k])
    )
    ideal_len = min(len(rel), k)
    idcg = sum(1.0 / math.log2(i + 2) for i in range(ideal_len))
    return dcg / idcg if idcg else 0.0


def hit_at_k(ranked: Sequence[str], rel: set[str], k: int) -> float:
    """Return 1.0 if any of the top-k results is relevant, else 0.0."""
    return 1.0 if set(ranked[:k]) & rel else 0.0


def recall_at_k(ranked: Sequence[str], rel: set[str], k: int) -> float:
    """Fraction of the relevant set covered by the top-k results.

    Returns 0.0 when rel is empty (no relevant items → no recall to measure).
    """
    if not rel:
        return 0.0
    return len(set(ranked[:k]) & rel) / len(rel)


def precision_at_1(ranked: Sequence[str], rel: set[str]) -> float:
    """Return 1.0 if the top result is relevant, else 0.0."""
    return 1.0 if ranked and ranked[0] in rel else 0.0


# ── Candidate-recall ─────────────────────────────────────────────────────────

def candidate_recall_at_limit(ranked_pool: Sequence[str],
                               gold_names: set[str]) -> float:
    """Fraction of gold (relevant) skills present anywhere in the returned pool.

    This is the PRIMARY signal that candidate generation can move.  At the
    262-skill corpus scale, the ranking brain (eq.3 dense cosine) is identical
    across all backends; the only lever each backend controls is which candidates
    it proposes.  If a gold skill is not in the pool, no ranking algorithm can
    rescue it.

    Call with the result of ``find_skill(prompt, limit=candidate_limit)`` where
    candidate_limit is large (e.g. 50) so the pool is the full candidate set.

    Args:
        ranked_pool: all candidates returned at the large candidate_limit.
        gold_names:  set of skill names known to be relevant for this query.
                     Typically {anchor} ∪ judge-relevant siblings.
    """
    if not gold_names:
        return 0.0
    present = sum(1 for name in gold_names if name in set(ranked_pool))
    return present / len(gold_names)


# ── Paired comparison ────────────────────────────────────────────────────────

def paired_rank_diffs(arm_a_first_ranks: Sequence[int],
                      arm_b_first_ranks: Sequence[int]) -> dict:
    """Per-query paired rank-difference summary between two sweep arms.

    For each query, the "first relevant rank" is the 1-indexed position of the
    first relevant result (use a large sentinel — e.g. 999 — for "not found").
    A lower rank is better.

    Returns:
        n              — total queries compared
        n_a_better     — queries where arm A has a strictly lower rank (better)
        n_b_better     — queries where arm B has a strictly lower rank (better)
        n_tie          — queries where ranks are equal
        mean_rank_delta — mean(rank_a - rank_b) over all queries
                         (negative = A is better on average)

    Args:
        arm_a_first_ranks: per-query first-relevant rank for arm A.
        arm_b_first_ranks: per-query first-relevant rank for arm B (same order).
    """
    if len(arm_a_first_ranks) != len(arm_b_first_ranks):
        raise ValueError(
            f"arm rank lists have different lengths: "
            f"{len(arm_a_first_ranks)} vs {len(arm_b_first_ranks)}"
        )
    n = len(arm_a_first_ranks)
    n_a_better = sum(1 for a, b in zip(arm_a_first_ranks, arm_b_first_ranks) if a < b)
    n_b_better = sum(1 for a, b in zip(arm_a_first_ranks, arm_b_first_ranks) if b < a)
    n_tie = n - n_a_better - n_b_better
    deltas = [a - b for a, b in zip(arm_a_first_ranks, arm_b_first_ranks)]
    mean_delta = sum(deltas) / n if n else 0.0
    return {
        "n": n,
        "n_a_better": n_a_better,
        "n_b_better": n_b_better,
        "n_tie": n_tie,
        "mean_rank_delta": mean_delta,
    }


def sign_test(n_a_better: int, n_b_better: int) -> float:
    """Two-sided exact binomial sign test p-value over the discordant query pairs.

    Ties (equal ranks) are excluded from the test (only discordant pairs matter).
    Under H0, each discordant pair is equally likely to favour A or B (p=0.5).

    Formula (two-sided exact binomial):
        n_disc = n_a_better + n_b_better   (discordant pairs only)
        p_value = 2 * sum_{i=0}^{k} C(n_disc, i) * 0.5^n_disc
        where k = min(n_a_better, n_b_better)
        (the smaller tail sum, then doubled for two-sided; capped at 1.0)

    Uses math.comb; no scipy.

    Returns the p-value in [0, 1].  Small p (< 0.05) means the difference is
    statistically significant.

    Args:
        n_a_better: number of queries where arm A ranked first-relevant higher.
        n_b_better: number of queries where arm B ranked first-relevant higher.
    """
    n_disc = n_a_better + n_b_better
    if n_disc == 0:
        # No discordant pairs — cannot reject H0; p = 1.0 (indistinguishable).
        return 1.0
    k_min = min(n_a_better, n_b_better)
    # One-sided tail probability: P(X <= k_min) under Binomial(n_disc, 0.5).
    half_pow = 0.5 ** n_disc
    tail = sum(math.comb(n_disc, i) * half_pow for i in range(k_min + 1))
    # Two-sided: multiply by 2, cap at 1.0 (in case of near-symmetric case).
    return min(1.0, 2.0 * tail)


# ── Score-distribution histogram ─────────────────────────────────────────────

def histogram(scores: Sequence[float], bins: int = 10) -> dict:
    """Bucket float scores into `bins` equal-width buckets and return stats.

    The bucket range spans [min(scores), max(scores)] divided into `bins`
    equal-width intervals.  The last bucket is right-inclusive so the maximum
    score always falls in the last bucket.

    Returns a dict with:
        counts   — list[int] of length `bins`; sum equals len(scores)
        edges    — list[float] of length `bins` + 1 (left edges + rightmost edge)
        min      — float
        max      — float
        mean     — float
        median   — float
        p10      — 10th percentile (nearest-rank)
        p90      — 90th percentile (nearest-rank)

    Raises:
        ValueError: if scores is empty or bins < 1.
    """
    if not scores:
        raise ValueError("histogram requires at least one score")
    if bins < 1:
        raise ValueError(f"bins must be >= 1, got {bins}")

    sorted_scores = sorted(scores)
    n = len(sorted_scores)
    lo, hi = sorted_scores[0], sorted_scores[-1]

    # Build equal-width edges.
    span = hi - lo
    if span == 0.0:
        # All scores identical — put everything in a single bin with zero width.
        edges = [lo] * bins + [lo]
        counts = [0] * bins
        counts[0] = n
    else:
        step = span / bins
        edges = [lo + i * step for i in range(bins)] + [hi]
        counts = [0] * bins
        for s in scores:
            # Find the bucket index; clamp the maximum into the last bucket.
            idx = min(int((s - lo) / step), bins - 1)
            counts[idx] += 1

    def _percentile(p: float) -> float:
        """Nearest-rank percentile (same method as retrieval_quality_live.py)."""
        idx = max(0, min(n - 1, int(math.ceil(p / 100.0 * n)) - 1))
        return sorted_scores[idx]

    total = sum(sorted_scores)
    return {
        "counts": counts,
        "edges": edges,
        "min": lo,
        "max": hi,
        "mean": total / n,
        "median": _percentile(50),
        "p10": _percentile(10),
        "p90": _percentile(90),
    }


# ── α=0 crater check ─────────────────────────────────────────────────────────

def crater_check(baseline_mrr: float, control_mrr: float) -> dict:
    """Check whether the α=0 control arm craters MRR relative to the baseline.

    Setting RETRIEVAL_ALPHA=0.0 removes the dense cosine signal entirely, so
    a healthy fixture should show a large relative MRR drop.  If it does NOT
    crater, the fixture cannot discriminate — the sweep verdict is void.

    Relative drop = (baseline - control) / baseline.
    A crater is defined as rel_drop >= 0.50 (50% relative drop).

    Args:
        baseline_mrr: MRR of the reference arm (snapshot_dense, alpha default).
        control_mrr:  MRR of the α=0 control arm (RETRIEVAL_ALPHA=0.0).

    Returns a dict with:
        abs_drop  — baseline_mrr - control_mrr (absolute)
        rel_drop  — relative drop (0.0 when baseline_mrr == 0)
        craters   — True when rel_drop >= 0.50
    """
    if baseline_mrr == 0.0:
        return {"abs_drop": 0.0, "rel_drop": 0.0, "craters": False}
    abs_drop = baseline_mrr - control_mrr
    rel_drop = abs_drop / baseline_mrr
    return {
        "abs_drop": round(abs_drop, 6),
        "rel_drop": round(rel_drop, 6),
        "craters": rel_drop >= 0.50,
    }


# ── Self-test ─────────────────────────────────────────────────────────────────

def _assert(condition: bool, label: str, detail: str = "") -> bool:
    """Print PASS or FAIL for a single named assertion.  Returns the result."""
    status = "PASS" if condition else "FAIL"
    suffix = f"  [{detail}]" if detail else ""
    print(f"  {status}  {label}{suffix}")
    return condition


def _run_self_tests() -> int:
    """Run all self-test cases.  Return 0 if all pass, 1 if any fail."""
    print("=== t11_metrics self-test ===")
    failures = 0

    # ── reciprocal_rank / mrr_at_k ─────────────────────────────────────────
    print("\n-- MRR@k --")

    # First relevant at rank 1 (within k=3).
    rr = reciprocal_rank(["A", "B", "C"], {"A"})
    ok = _assert(rr == 1.0, "rr: first at rank 1", f"got {rr}")
    failures += 0 if ok else 1

    # First relevant at rank 2.
    rr = reciprocal_rank(["X", "A", "C"], {"A"})
    ok = _assert(abs(rr - 0.5) < 1e-9, "rr: first at rank 2", f"got {rr}")
    failures += 0 if ok else 1

    # Not found → 0.
    rr = reciprocal_rank(["X", "Y", "Z"], {"A"})
    ok = _assert(rr == 0.0, "rr: not found → 0.0", f"got {rr}")
    failures += 0 if ok else 1

    # MRR@3: first relevant at rank 4 — beyond window → contributes 0.
    mrr = mrr_at_k([["X", "Y", "Z", "A"]], [{"A"}], k=3)
    ok = _assert(mrr == 0.0, "mrr@3: first-relevant beyond k=3 → 0", f"got {mrr}")
    failures += 0 if ok else 1

    # MRR@10: first relevant at rank 4 — within window → contributes 1/4.
    mrr = mrr_at_k([["X", "Y", "Z", "A"]], [{"A"}], k=10)
    ok = _assert(abs(mrr - 0.25) < 1e-9, "mrr@10: first-relevant at rank 4 → 0.25", f"got {mrr}")
    failures += 0 if ok else 1

    # MRR over multiple queries: [rank-1, rank-2, miss] → (1 + 0.5 + 0) / 3.
    mrr = mrr_at_k(
        [["A", "X"], ["X", "A"], ["X", "Y"]],
        [{"A"}, {"A"}, {"A"}],
        k=3,
    )
    expected = (1.0 + 0.5 + 0.0) / 3
    ok = _assert(abs(mrr - expected) < 1e-9, "mrr@3 multi-query", f"got {mrr}, expected {expected}")
    failures += 0 if ok else 1

    # ── nDCG@3 ─────────────────────────────────────────────────────────────
    print("\n-- nDCG@3 --")

    # Perfect: relevant at rank 1 of 1.
    nd = ndcg_at_k(["A"], {"A"}, 3)
    ok = _assert(nd == 1.0, "ndcg@3: single relevant at rank 1 → 1.0", f"got {nd}")
    failures += 0 if ok else 1

    # Empty rel set → 0.
    nd = ndcg_at_k(["A", "B"], set(), 3)
    ok = _assert(nd == 0.0, "ndcg@3: empty rel → 0.0", f"got {nd}")
    failures += 0 if ok else 1

    # Relevant at rank 2 only.
    nd = ndcg_at_k(["X", "A"], {"A"}, 3)
    expected_nd = (1.0 / math.log2(3)) / (1.0 / math.log2(2))
    ok = _assert(abs(nd - expected_nd) < 1e-9, "ndcg@3: relevant at rank 2", f"got {nd} expected {expected_nd}")
    failures += 0 if ok else 1

    # ── hit@k / recall@k / precision@1 ────────────────────────────────────
    print("\n-- hit@k / recall@k / precision@1 --")

    ok = _assert(hit_at_k(["X", "A", "B"], {"A"}, 3) == 1.0, "hit@3: relevant in top-3 → 1")
    failures += 0 if ok else 1

    ok = _assert(hit_at_k(["X", "Y", "Z"], {"A"}, 3) == 0.0, "hit@3: not in top-3 → 0")
    failures += 0 if ok else 1

    ok = _assert(abs(recall_at_k(["A", "B", "X"], {"A", "B", "C"}, 3) - 2/3) < 1e-9,
                 "recall@3: 2 of 3 relevant in top-3", f"got {recall_at_k(['A','B','X'], {'A','B','C'}, 3)}")
    failures += 0 if ok else 1

    ok = _assert(recall_at_k(["A"], set(), 3) == 0.0, "recall@3: empty rel → 0")
    failures += 0 if ok else 1

    ok = _assert(precision_at_1(["A", "B"], {"A"}) == 1.0, "p@1: first relevant → 1")
    failures += 0 if ok else 1

    ok = _assert(precision_at_1(["X", "A"], {"A"}) == 0.0, "p@1: first not relevant → 0")
    failures += 0 if ok else 1

    ok = _assert(precision_at_1([], {"A"}) == 0.0, "p@1: empty ranked → 0")
    failures += 0 if ok else 1

    # ── candidate_recall_at_limit ──────────────────────────────────────────
    print("\n-- candidate_recall_at_limit --")

    # All gold names in pool.
    cr = candidate_recall_at_limit(["A", "B", "C", "D"], {"A", "B"})
    ok = _assert(cr == 1.0, "candidate_recall: all gold in pool → 1.0", f"got {cr}")
    failures += 0 if ok else 1

    # Partial: only one of two gold names in pool.
    cr = candidate_recall_at_limit(["A", "X", "Y"], {"A", "Z"})
    ok = _assert(cr == 0.5, "candidate_recall: partial gold in pool → 0.5", f"got {cr}")
    failures += 0 if ok else 1

    # Zero: no gold names in pool.
    cr = candidate_recall_at_limit(["X", "Y"], {"A", "B"})
    ok = _assert(cr == 0.0, "candidate_recall: zero gold in pool → 0.0", f"got {cr}")
    failures += 0 if ok else 1

    # Empty gold → 0.
    cr = candidate_recall_at_limit(["A", "B"], set())
    ok = _assert(cr == 0.0, "candidate_recall: empty gold → 0.0", f"got {cr}")
    failures += 0 if ok else 1

    # ── paired_rank_diffs ──────────────────────────────────────────────────
    print("\n-- paired_rank_diffs --")

    diff = paired_rank_diffs([1, 2, 3], [2, 2, 4])
    ok = _assert(diff["n"] == 3, "paired: n=3", f"got {diff['n']}")
    failures += 0 if ok else 1

    ok = _assert(diff["n_a_better"] == 2, "paired: A wins 2 (ranks 1<2, 3<4)", f"got {diff['n_a_better']}")
    failures += 0 if ok else 1

    ok = _assert(diff["n_tie"] == 1, "paired: 1 tie (rank 2==2)", f"got {diff['n_tie']}")
    failures += 0 if ok else 1

    expected_delta = ((1-2) + (2-2) + (3-4)) / 3
    ok = _assert(abs(diff["mean_rank_delta"] - expected_delta) < 1e-9,
                 "paired: mean_rank_delta", f"got {diff['mean_rank_delta']} expected {expected_delta}")
    failures += 0 if ok else 1

    # ── sign_test ──────────────────────────────────────────────────────────
    print("\n-- sign_test --")

    # Clearly skewed: A wins 9, B wins 1 → p should be small.
    p = sign_test(9, 1)
    ok = _assert(p < 0.05, f"sign_test: 9 vs 1 → p < 0.05", f"got p={p:.4f}")
    failures += 0 if ok else 1

    # Balanced: A wins 5, B wins 5 → p should be ~1.0 (cannot reject H0).
    p = sign_test(5, 5)
    ok = _assert(p > 0.5, f"sign_test: 5 vs 5 → p > 0.5 (balanced)", f"got p={p:.4f}")
    failures += 0 if ok else 1

    # All ties: 0 vs 0 → p = 1.0.
    p = sign_test(0, 0)
    ok = _assert(p == 1.0, "sign_test: 0 vs 0 (all ties) → p = 1.0", f"got {p}")
    failures += 0 if ok else 1

    # Extreme: A wins all 10 → p should be very small (2 * 0.5^10 = ~0.002).
    p = sign_test(10, 0)
    expected_p = 2.0 * (0.5 ** 10)
    ok = _assert(abs(p - expected_p) < 1e-9, f"sign_test: 10 vs 0 → {expected_p:.6f}", f"got {p:.6f}")
    failures += 0 if ok else 1

    # ── histogram ──────────────────────────────────────────────────────────
    print("\n-- histogram --")

    scores = [0.1, 0.3, 0.5, 0.7, 0.9, 0.2, 0.4, 0.6, 0.8, 1.0]
    h = histogram(scores, bins=5)

    ok = _assert(sum(h["counts"]) == len(scores),
                 "histogram: bucket counts sum to len(scores)",
                 f"sum={sum(h['counts'])} len={len(scores)}")
    failures += 0 if ok else 1

    ok = _assert(len(h["counts"]) == 5, "histogram: 5 buckets", f"got {len(h['counts'])}")
    failures += 0 if ok else 1

    ok = _assert(abs(h["min"] - 0.1) < 1e-9, "histogram: min", f"got {h['min']}")
    failures += 0 if ok else 1

    ok = _assert(abs(h["max"] - 1.0) < 1e-9, "histogram: max", f"got {h['max']}")
    failures += 0 if ok else 1

    # All-identical scores → single bin has all counts.
    h2 = histogram([0.5, 0.5, 0.5], bins=3)
    ok = _assert(sum(h2["counts"]) == 3, "histogram: identical scores, counts sum to 3", f"got {sum(h2['counts'])}")
    failures += 0 if ok else 1

    # ── crater_check ───────────────────────────────────────────────────────
    print("\n-- crater_check --")

    # 60% relative drop → craters.
    c = crater_check(0.80, 0.32)
    ok = _assert(c["craters"] is True,
                 "crater_check: 60% drop craters",
                 f"rel_drop={c['rel_drop']:.3f} craters={c['craters']}")
    failures += 0 if ok else 1

    # 20% relative drop → does NOT crater.
    c = crater_check(0.80, 0.64)
    ok = _assert(c["craters"] is False,
                 "crater_check: 20% drop does NOT crater",
                 f"rel_drop={c['rel_drop']:.3f} craters={c['craters']}")
    failures += 0 if ok else 1

    # Zero baseline guard.
    c = crater_check(0.0, 0.0)
    ok = _assert(c["craters"] is False, "crater_check: zero baseline → no crater", f"got {c}")
    failures += 0 if ok else 1

    # ── summary ────────────────────────────────────────────────────────────
    print(f"\n{'=' * 40}")
    if failures == 0:
        print("ALL TESTS PASSED")
    else:
        print(f"{failures} TEST(S) FAILED", file=sys.stderr)
    return 0 if failures == 0 else 1


def main() -> None:
    """CLI entry point.  Only ``--self-test`` is meaningful for this module."""
    ap = argparse.ArgumentParser(
        description="T11 pure metric functions.  Run --self-test to validate."
    )
    ap.add_argument(
        "--self-test",
        action="store_true",
        help="Run synthetic self-tests for all metric functions and exit.",
    )
    args = ap.parse_args()

    if args.self_test:
        sys.exit(_run_self_tests())
    else:
        ap.print_help()
        sys.exit(0)


if __name__ == "__main__":
    main()
