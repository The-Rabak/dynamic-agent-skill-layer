#!/usr/bin/env python3
"""Retrieval measurement instruments — pure metric functions for the 262-corpus sweep/gate.

WHY this module exists
----------------------
The T11 sweep (2026-06-11) revealed that at 262-skill corpus scale the ranking
brain (eq.3 dense cosine) is identical across all backends; only *candidate
generation* differs.  That means only *candidate-recall* can move — not top-3
ranking, which is quantized and saturated.

The instruments required to produce a real verdict are therefore:
  - candidate_recall_at_limit  — the ONLY metric candidate-gen can move
  - paired per-query rank diffs + sign test — mean equality at 3 decimals is
    NOT a verdict; we need a per-query paired comparison
  - α=0 crater check — proves the fixture can discriminate at all; if α=0
    does NOT crater MRR the fixture is broken, not the backend

The module also exposes ``GATE_THRESHOLDS`` (used by ``retrieval_sweep.py
--gate``) and supports gate-decision self-tests via ``--self-test``.

All functions are pure (no I/O, no server calls, no state).  The live sweep
orchestrator (retrieval_sweep.py) calls these after collecting server responses.

Formulas match scripts/retrieval_quality_live.py exactly so numbers are
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

def percentile_nearest_rank(sorted_samples: list, p: float) -> float:
    """Nearest-rank percentile over an already-sorted sample list.

    Uses the nearest-rank formula: rank = ceil(p/100 * n), clamped to [1, n].
    Returns 0.0 for an empty list.  Matches the formula used historically in
    retrieval_quality_live.py's latency percentile reporting.

    Args:
        sorted_samples: list of numeric values sorted in ascending order.
        p:              percentile in [0, 100] (e.g. 95 for p95).
    """
    if not sorted_samples:
        return 0.0
    rank = max(1, math.ceil(p / 100.0 * len(sorted_samples)))
    return sorted_samples[rank - 1]


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

    total = sum(sorted_scores)
    return {
        "counts": counts,
        "edges": edges,
        "min": lo,
        "max": hi,
        "mean": total / n,
        "median": percentile_nearest_rank(sorted_scores, 50),
        "p10": percentile_nearest_rank(sorted_scores, 10),
        "p90": percentile_nearest_rank(sorted_scores, 90),
    }


# ── Priming metrics (T18) ────────────────────────────────────────────────────

def set_coverage_at_n(injected_names: Sequence[str],
                      gold_set: set[str],
                      n: int) -> float:
    """Fraction of the gold set covered by the top-N injected skill names.

    This is the headline priming metric.  Unlike MRR (single-gold, rank-ordered),
    set-coverage measures how much of a *multi-gold* relevant set the bounded
    prime surfaces.  A score of 1.0 means every skill the prime should surface
    appears in the first N injected skills.

    Returns 0.0 when gold_set is empty (no coverage to measure).

    Args:
        injected_names: ordered list of skill names as injected by compile_context
                        (position 0 = first injected).  Only the first ``n``
                        entries are considered.
        gold_set:       set of skill names that constitute the gold prime for
                        this query (multi-gold, from the session_start stratum).
        n:              injection window size (e.g. 3 for the production cap).
    """
    if not gold_set:
        return 0.0
    top_n = set(injected_names[:n])
    return len(top_n & gold_set) / len(gold_set)


def freshness_hit_rate(injected_names: Sequence[str],
                       fresh_golds: Sequence[str]) -> float | None:
    """Whether ≥1 fresh-gold skill appears anywhere in the injected set.

    The freshness slot (T12) targets high-value, brand-new / low-prior-use
    skills.  This metric measures whether the current prime surfaces at least
    one of them.  It is defined *only* over queries that have ≥1 fresh gold;
    callers must aggregate the non-None values themselves and report the
    denominator clearly.

    Returns:
        1.0 if any fresh_gold name appears in injected_names.
        0.0 if none do.
        None if fresh_golds is empty (metric undefined for this query — exclude
        from mean; the caller must not count None towards the denominator).

    Args:
        injected_names: all skill names injected by compile_context (any depth).
        fresh_golds:    the subset of the gold set tagged as high-value / fresh.
    """
    if not fresh_golds:
        return None
    injected_set = set(injected_names)
    return 1.0 if any(name in injected_set for name in fresh_golds) else 0.0


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


# ── Gate thresholds + decision ───────────────────────────────────────────────

# Floors derived from T11-measured anchor-only numbers (T11-VALIDATION-REPORT.md §2).
#
# Rationale for floor placement:
#   The T11 sweep measured two distinct dense operating points:
#     - dense_views ON  (current default): MRR@3 0.743, cand-recall@50 0.796, nDCG@3 0.755, no_match 0.92
#     - single-view dense (flag OFF):      MRR@3 0.686, cand-recall@50 0.723, nDCG@3 0.696
#
#   Floors are set BELOW the single-view dense numbers so the gate:
#     a) does not fire when dense_views is temporarily OFF (floor robustness)
#     b) DOES fire on a genuine regression (meaningful drop below the worst measured arm)
#
#   Each floor = T11 single-view measured value − margin.
#   Margins are per-metric (see the per-line comments): mrr_at3 0.046, mrr_at10 0.046,
#   ndcg_at3 0.056, cand-recall 0.043, no_match 0.04 — roughly 0.043–0.056 of slack
#   below each single-view measured value.
GATE_THRESHOLDS: dict[str, float] = {
    # T11 measured single-view dense: 0.686.  Floor = 0.686 − 0.046 = 0.640.
    "mrr_at3": 0.64,
    # T11 measured single-view dense: 0.686 (MRR@3 == MRR@10 for every arm —
    # first relevant hit is always top-3 or absent from top-10; floor matches mrr_at3).
    "mrr_at10": 0.64,
    # T11 measured single-view dense: 0.696.  Floor = 0.696 − 0.056 = 0.640.
    "ndcg_at3": 0.64,
    # T11 measured single-view dense: 0.723.  Floor = 0.723 − 0.043 = 0.680.
    # This is the LEVER metric; extra care to keep the floor meaningful.
    "candidate_recall_at_limit": 0.68,
    # T11 measured (all arms): 0.92.  Floor = 0.92 − 0.04 = 0.880.
    "no_match_precision": 0.88,
}


def gate_decision(baseline_metrics: dict, alpha0_metrics: dict) -> dict:
    """Evaluate whether a gate run passes all floor assertions and the α=0 crater canary.

    ``baseline_metrics`` is the metric dict from the baseline/dense_views arm as
    returned by ``_compute_arm_metrics`` (keys: mrr_at3, mrr_at10, ndcg_at3,
    candidate_recall_at_limit, no_match_precision).  ``alpha0_metrics`` only needs
    the ``mrr_at3`` key.

    Each floor in ``GATE_THRESHOLDS`` is compared against the corresponding metric.
    The α=0 crater is checked via ``crater_check`` (≥50% relative MRR drop).

    Returns a dict with:
        passed    — True iff ALL floor assertions AND the crater canary pass.
        failures  — list of failure descriptions (empty when passed is True).
        assertions — per-assertion pass/fail records for the JSON gate report.
    """
    failures: list[str] = []
    assertions: list[dict] = []

    for metric_key, floor in GATE_THRESHOLDS.items():
        got = baseline_metrics.get(metric_key)
        if got is None:
            msg = f"{metric_key}: missing from arm metrics (cannot evaluate floor {floor})"
            failures.append(msg)
            assertions.append({"metric": metric_key, "floor": floor, "got": None, "passed": False, "detail": msg})
            continue
        passes = got >= floor
        if not passes:
            failures.append(
                f"{metric_key}: {got:.4f} < floor {floor:.4f}"
            )
        assertions.append({
            "metric": metric_key,
            "floor": floor,
            "got": round(got, 6),
            "passed": passes,
        })

    # α=0 crater canary: if it does NOT crater MRR ≥50%, the fixture has drifted
    # and every gate verdict is void — treat as a gate failure.
    baseline_mrr = baseline_metrics.get("mrr_at3", 0.0)
    alpha0_mrr = alpha0_metrics.get("mrr_at3", 0.0)
    crater = crater_check(baseline_mrr, alpha0_mrr)
    crater_passes = crater["craters"]
    if not crater_passes:
        failures.append(
            f"alpha0_crater: MRR relative drop {crater['rel_drop']:.3f} < 0.50 "
            f"(baseline {baseline_mrr:.4f}, alpha0 {alpha0_mrr:.4f}). "
            f"Fixture may have drifted — gate verdict is void."
        )
    assertions.append({
        "metric": "alpha0_crater",
        "required_rel_drop": 0.50,
        "baseline_mrr": baseline_mrr,
        "alpha0_mrr": alpha0_mrr,
        "rel_drop": crater["rel_drop"],
        "passed": crater_passes,
    })

    return {
        "passed": len(failures) == 0,
        "failures": failures,
        "assertions": assertions,
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
    print("=== retrieval_metrics self-test ===")
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

    # ── percentile_nearest_rank ────────────────────────────────────────────
    print("\n-- percentile_nearest_rank --")

    # Empty list → 0.0.
    ok = _assert(percentile_nearest_rank([], 50) == 0.0, "percentile_nearest_rank: empty list → 0.0")
    failures += 0 if ok else 1

    # p50 of [1, 2, 3, 4] = rank ceil(2.0)=2 → value at index 1 = 2.
    ok = _assert(percentile_nearest_rank([1, 2, 3, 4], 50) == 2, "percentile_nearest_rank: p50 of [1,2,3,4] → 2")
    failures += 0 if ok else 1

    # p100 of [10, 20, 30] = rank ceil(3.0)=3 → value at index 2 = 30.
    ok = _assert(percentile_nearest_rank([10, 20, 30], 100) == 30, "percentile_nearest_rank: p100 → max element")
    failures += 0 if ok else 1

    # p0 of [10, 20, 30] = rank max(1, ceil(0))=1 → value at index 0 = 10.
    ok = _assert(percentile_nearest_rank([10, 20, 30], 0) == 10, "percentile_nearest_rank: p0 → first element")
    failures += 0 if ok else 1

    # p95 of 100 equally-spaced samples: rank=ceil(95)=95 → 95th element (0-indexed: 94).
    samples_100 = list(range(1, 101))
    ok = _assert(percentile_nearest_rank(samples_100, 95) == 95, "percentile_nearest_rank: p95 of 1..100 → 95")
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

    # ── set_coverage_at_n ─────────────────────────────────────────────────
    print("\n-- set_coverage_at_n --")

    # All gold covered in top-3 → 1.0.
    cov = set_coverage_at_n(["A", "B", "C", "D"], {"A", "B"}, 3)
    ok = _assert(abs(cov - 1.0) < 1e-9,
                 "set_coverage@3: both gold in top-3 → 1.0",
                 f"got {cov}")
    failures += 0 if ok else 1

    # Half coverage: 1 of 2 gold in top-3.
    cov = set_coverage_at_n(["A", "X", "Y", "B"], {"A", "B"}, 3)
    ok = _assert(abs(cov - 0.5) < 1e-9,
                 "set_coverage@3: 1 of 2 gold in top-3 → 0.5",
                 f"got {cov}")
    failures += 0 if ok else 1

    # Gold beyond N window → 0.0.
    cov = set_coverage_at_n(["X", "Y", "Z", "A"], {"A"}, 3)
    ok = _assert(cov == 0.0,
                 "set_coverage@3: gold only at rank 4 → 0.0",
                 f"got {cov}")
    failures += 0 if ok else 1

    # Empty gold set → 0.0.
    cov = set_coverage_at_n(["A", "B"], set(), 3)
    ok = _assert(cov == 0.0,
                 "set_coverage@3: empty gold → 0.0",
                 f"got {cov}")
    failures += 0 if ok else 1

    # Empty injected, non-empty gold → 0.0.
    cov = set_coverage_at_n([], {"A", "B"}, 3)
    ok = _assert(cov == 0.0,
                 "set_coverage@3: empty injected → 0.0",
                 f"got {cov}")
    failures += 0 if ok else 1

    # ── freshness_hit_rate ─────────────────────────────────────────────────
    print("\n-- freshness_hit_rate --")

    # Fresh gold appears in injected → 1.0.
    fhr = freshness_hit_rate(["A", "B", "C"], ["B"])
    ok = _assert(fhr == 1.0,
                 "freshness_hit_rate: fresh gold in injected → 1.0",
                 f"got {fhr}")
    failures += 0 if ok else 1

    # Fresh gold NOT in injected → 0.0.
    fhr = freshness_hit_rate(["X", "Y", "Z"], ["B"])
    ok = _assert(fhr == 0.0,
                 "freshness_hit_rate: fresh gold absent → 0.0",
                 f"got {fhr}")
    failures += 0 if ok else 1

    # Multiple fresh golds, one present → 1.0.
    fhr = freshness_hit_rate(["A", "X"], ["B", "A"])
    ok = _assert(fhr == 1.0,
                 "freshness_hit_rate: one of two fresh golds present → 1.0",
                 f"got {fhr}")
    failures += 0 if ok else 1

    # Empty fresh_golds → None (metric undefined).
    fhr = freshness_hit_rate(["A", "B"], [])
    ok = _assert(fhr is None,
                 "freshness_hit_rate: empty fresh_golds → None",
                 f"got {fhr}")
    failures += 0 if ok else 1

    # Empty injected with fresh golds → 0.0.
    fhr = freshness_hit_rate([], ["A"])
    ok = _assert(fhr == 0.0,
                 "freshness_hit_rate: empty injected, fresh gold present → 0.0",
                 f"got {fhr}")
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
    # ── gate_decision ──────────────────────────────────────────────────────
    print("\n-- gate_decision --")

    # Arm above all floors + craters properly → PASS.
    synthetic_above = {
        "mrr_at3": 0.70,
        "mrr_at10": 0.70,
        "ndcg_at3": 0.70,
        "candidate_recall_at_limit": 0.75,
        "no_match_precision": 0.93,
    }
    synthetic_alpha0 = {"mrr_at3": 0.05}
    result = gate_decision(synthetic_above, synthetic_alpha0)
    ok = _assert(
        result["passed"] is True,
        "gate_decision: all-above floors + 93% crater → PASS",
        f"passed={result['passed']} failures={result['failures']}",
    )
    failures += 0 if ok else 1

    # Arm with one metric below floor → FAIL.
    synthetic_below = {
        "mrr_at3": 0.60,   # below floor of 0.64
        "mrr_at10": 0.70,
        "ndcg_at3": 0.70,
        "candidate_recall_at_limit": 0.75,
        "no_match_precision": 0.93,
    }
    result = gate_decision(synthetic_below, synthetic_alpha0)
    ok = _assert(
        result["passed"] is False and any("mrr_at3" in f for f in result["failures"]),
        "gate_decision: mrr_at3 below floor → FAIL with mrr_at3 in failures",
        f"passed={result['passed']} failures={result['failures']}",
    )
    failures += 0 if ok else 1

    # α=0 crater canary: only 10% relative MRR drop (far below the 50% threshold) → canary FAIL.
    synthetic_no_crater = {"mrr_at3": 0.75}   # only 6% drop from 0.70
    result = gate_decision(synthetic_above, synthetic_no_crater)
    ok = _assert(
        result["passed"] is False and any("alpha0_crater" in f for f in result["failures"]),
        "gate_decision: 6% crater far below 50% threshold → FAIL with alpha0_crater in failures",
        f"passed={result['passed']} failures={result['failures']}",
    )
    failures += 0 if ok else 1

    # α=0 100% crater (real T11 result) → canary PASS.
    synthetic_full_crater = {"mrr_at3": 0.000}
    result = gate_decision(synthetic_above, synthetic_full_crater)
    ok = _assert(
        result["passed"] is True,
        "gate_decision: 100% crater → canary PASS, overall PASS",
        f"passed={result['passed']} failures={result['failures']}",
    )
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
        description="Retrieval pure metric functions.  Run --self-test to validate."
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
