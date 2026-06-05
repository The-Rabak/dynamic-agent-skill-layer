//! Information-retrieval quality metrics over a ranked result list and a set of
//! ground-truth relevant ids (binary relevance).
//!
//! These are PURE functions with no infrastructure dependency, so their
//! `#[cfg(test)]` unit tests run under a plain `cargo test` (no containers).
//! The live retrieval-quality harness (`test_retrieval_quality.rs`) feeds them
//! the ranking parsed from a REAL `compile_context` response and the labels from
//! `tests/fixtures/retrieval_quality_labeled.json`.
//!
//! Binary relevance is the right model here: each labeled query names the skill
//! id(s) that are relevant, with no graded scale, so gain ∈ {0, 1}.
#![allow(dead_code)]

use std::collections::BTreeSet;

/// Per-query metric bundle computed from one ranked id list against one relevant set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QueryMetrics {
    /// Fraction of the top-k that are relevant.
    pub precision_at_k: f64,
    /// Fraction of relevant items that appear in the top-k.
    pub recall_at_k: f64,
    /// 1/rank of the first relevant item (0 if none in the ranking).
    pub reciprocal_rank: f64,
    /// Average precision over the ranking (the AP that MAP averages).
    pub average_precision: f64,
    /// Normalized discounted cumulative gain at k (binary gain).
    pub ndcg_at_k: f64,
    /// `true` when at least one relevant id appears anywhere in the ranking.
    pub hit: bool,
}

/// Computes the full metric bundle for one ranked list at cutoff `k`.
///
/// `ranked` is the ordered list of returned ids (rank 1 first). `relevant` is the
/// ground-truth set. Duplicate ids in `ranked` are collapsed to their first
/// occurrence so a result list that repeats an id cannot inflate precision.
pub fn query_metrics(ranked: &[String], relevant: &BTreeSet<String>, k: usize) -> QueryMetrics {
    let deduped = dedupe_preserving_order(ranked);

    QueryMetrics {
        precision_at_k: precision_at_k(&deduped, relevant, k),
        recall_at_k: recall_at_k(&deduped, relevant, k),
        reciprocal_rank: reciprocal_rank(&deduped, relevant),
        average_precision: average_precision(&deduped, relevant),
        ndcg_at_k: ndcg_at_k(&deduped, relevant, k),
        hit: deduped.iter().any(|id| relevant.contains(id)),
    }
}

/// Precision@k: relevant items in the top-k divided by k.
///
/// Divides by `k` (not by `min(k, len)`) so a short result list that returns
/// fewer than `k` items is honestly penalised rather than flattered.
pub fn precision_at_k(ranked: &[String], relevant: &BTreeSet<String>, k: usize) -> f64 {
    if k == 0 {
        return 0.0;
    }
    let hits = ranked
        .iter()
        .take(k)
        .filter(|id| relevant.contains(*id))
        .count();
    hits as f64 / k as f64
}

/// Recall@k: relevant items in the top-k divided by the total relevant count.
pub fn recall_at_k(ranked: &[String], relevant: &BTreeSet<String>, k: usize) -> f64 {
    if relevant.is_empty() {
        return 0.0;
    }
    let hits = ranked
        .iter()
        .take(k)
        .filter(|id| relevant.contains(*id))
        .count();
    hits as f64 / relevant.len() as f64
}

/// Reciprocal rank: 1/rank of the first relevant id, or 0.0 if none present.
///
/// The mean of this across queries is MRR.
pub fn reciprocal_rank(ranked: &[String], relevant: &BTreeSet<String>) -> f64 {
    for (idx, id) in ranked.iter().enumerate() {
        if relevant.contains(id) {
            return 1.0 / (idx as f64 + 1.0);
        }
    }
    0.0
}

/// Average precision over the full ranking: mean of precision@rank at each rank
/// where a relevant item appears, normalised by the number of relevant items.
///
/// The mean of this across queries is MAP.
pub fn average_precision(ranked: &[String], relevant: &BTreeSet<String>) -> f64 {
    if relevant.is_empty() {
        return 0.0;
    }
    let mut hits = 0usize;
    let mut sum_precision = 0.0;
    for (idx, id) in ranked.iter().enumerate() {
        if relevant.contains(id) {
            hits += 1;
            sum_precision += hits as f64 / (idx as f64 + 1.0);
        }
    }
    sum_precision / relevant.len() as f64
}

/// Normalised DCG at k with binary gain (gain 1 for relevant, 0 otherwise).
///
/// `dcg = Σ gain_i / log2(i + 1)`; the ideal DCG places all relevant items first.
pub fn ndcg_at_k(ranked: &[String], relevant: &BTreeSet<String>, k: usize) -> f64 {
    if relevant.is_empty() || k == 0 {
        return 0.0;
    }
    let dcg = ranked
        .iter()
        .take(k)
        .enumerate()
        .map(|(idx, id)| {
            let gain = if relevant.contains(id) { 1.0 } else { 0.0 };
            gain / ((idx as f64 + 2.0).log2())
        })
        .sum::<f64>();

    // Ideal DCG: as many relevant items as fit in k, all at the top.
    let ideal_hits = relevant.len().min(k);
    let idcg = (0..ideal_hits)
        .map(|idx| 1.0 / ((idx as f64 + 2.0).log2()))
        .sum::<f64>();

    if idcg == 0.0 { 0.0 } else { dcg / idcg }
}

/// Aggregate metrics over a set of per-query results.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AggregateMetrics {
    pub query_count: usize,
    pub mean_precision_at_1: f64,
    pub mean_precision_at_k: f64,
    pub mean_recall_at_k: f64,
    /// Mean reciprocal rank.
    pub mrr: f64,
    /// Mean average precision.
    pub map: f64,
    pub mean_ndcg_at_k: f64,
    /// Fraction of queries with at least one relevant id anywhere in the ranking.
    pub hit_rate: f64,
}

/// Averages a slice of `(ranked, relevant)` query results into one aggregate.
///
/// `precision_at_1` is reported separately because "is the very first result
/// right?" is the bar that matters most for a top-1-injected context layer.
pub fn aggregate(results: &[(Vec<String>, BTreeSet<String>)], k: usize) -> AggregateMetrics {
    if results.is_empty() {
        return AggregateMetrics {
            query_count: 0,
            mean_precision_at_1: 0.0,
            mean_precision_at_k: 0.0,
            mean_recall_at_k: 0.0,
            mrr: 0.0,
            map: 0.0,
            mean_ndcg_at_k: 0.0,
            hit_rate: 0.0,
        };
    }

    let n = results.len() as f64;
    let mut p1 = 0.0;
    let mut pk = 0.0;
    let mut rk = 0.0;
    let mut mrr = 0.0;
    let mut map = 0.0;
    let mut ndcg = 0.0;
    let mut hits = 0.0;

    for (ranked, relevant) in results {
        let m = query_metrics(ranked, relevant, k);
        p1 += precision_at_k(&dedupe_preserving_order(ranked), relevant, 1);
        pk += m.precision_at_k;
        rk += m.recall_at_k;
        mrr += m.reciprocal_rank;
        map += m.average_precision;
        ndcg += m.ndcg_at_k;
        if m.hit {
            hits += 1.0;
        }
    }

    AggregateMetrics {
        query_count: results.len(),
        mean_precision_at_1: p1 / n,
        mean_precision_at_k: pk / n,
        mean_recall_at_k: rk / n,
        mrr: mrr / n,
        map: map / n,
        mean_ndcg_at_k: ndcg / n,
        hit_rate: hits / n,
    }
}

/// Collapses repeated ids to their first occurrence, preserving rank order.
fn dedupe_preserving_order(ranked: &[String]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::with_capacity(ranked.len());
    for id in ranked {
        if seen.insert(id.clone()) {
            out.push(id.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(ids: &[&str]) -> BTreeSet<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    fn ranked(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn precision_at_k_divides_by_k_not_length() {
        // One relevant in the only returned slot, but k=3 → 1/3, not 1/1.
        let r = ranked(&["a"]);
        assert!((precision_at_k(&r, &rel(&["a"]), 3) - 1.0 / 3.0).abs() < 1e-9);
        // Top-1 of a perfect ranking is 1.0.
        assert!((precision_at_k(&ranked(&["a", "b", "c"]), &rel(&["a"]), 1) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn recall_at_k_counts_relevant_found() {
        let r = ranked(&["x", "a", "b"]);
        // 1 of 2 relevant found in top-3.
        assert!((recall_at_k(&r, &rel(&["a", "z"]), 3) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn reciprocal_rank_is_inverse_of_first_hit_position() {
        assert!((reciprocal_rank(&ranked(&["a", "b"]), &rel(&["a"])) - 1.0).abs() < 1e-9);
        assert!((reciprocal_rank(&ranked(&["x", "a"]), &rel(&["a"])) - 0.5).abs() < 1e-9);
        assert_eq!(reciprocal_rank(&ranked(&["x", "y"]), &rel(&["a"])), 0.0);
    }

    #[test]
    fn average_precision_rewards_early_hits() {
        // Relevant at ranks 1 and 3 of 3, two relevant total.
        // AP = (1/1 + 2/3) / 2 = (1.0 + 0.6667) / 2 = 0.8333.
        let ap = average_precision(&ranked(&["a", "x", "b"]), &rel(&["a", "b"]));
        assert!((ap - 0.833_333).abs() < 1e-4, "ap={ap}");
    }

    #[test]
    fn ndcg_is_one_for_ideal_ranking_and_less_for_worse() {
        let ideal = ndcg_at_k(&ranked(&["a", "x", "y"]), &rel(&["a"]), 3);
        assert!((ideal - 1.0).abs() < 1e-9, "ideal ndcg={ideal}");

        let worse = ndcg_at_k(&ranked(&["x", "y", "a"]), &rel(&["a"]), 3);
        assert!(worse < ideal && worse > 0.0, "worse ndcg={worse}");
    }

    #[test]
    fn negative_query_with_no_relevant_scores_zero_everywhere() {
        let m = query_metrics(&ranked(&["a", "b"]), &rel(&[]), 3);
        assert_eq!(m.precision_at_k, 0.0);
        assert_eq!(m.recall_at_k, 0.0);
        assert_eq!(m.reciprocal_rank, 0.0);
        assert_eq!(m.average_precision, 0.0);
        assert_eq!(m.ndcg_at_k, 0.0);
        assert!(!m.hit);
    }

    #[test]
    fn duplicate_ids_cannot_inflate_precision() {
        // A ranking that repeats the single relevant id must not score >1 relevant.
        let m = query_metrics(&ranked(&["a", "a", "a"]), &rel(&["a"]), 3);
        // After dedupe: ["a"] → precision@3 = 1/3.
        assert!((m.precision_at_k - 1.0 / 3.0).abs() < 1e-9, "p@k={}", m.precision_at_k);
    }

    #[test]
    fn aggregate_averages_across_queries() {
        let results = vec![
            (ranked(&["a", "x", "y"]), rel(&["a"])), // rr=1.0, p@1=1
            (ranked(&["x", "b", "y"]), rel(&["b"])), // rr=0.5, p@1=0
        ];
        let agg = aggregate(&results, 3);
        assert_eq!(agg.query_count, 2);
        assert!((agg.mrr - 0.75).abs() < 1e-9, "mrr={}", agg.mrr);
        assert!((agg.mean_precision_at_1 - 0.5).abs() < 1e-9);
        assert!((agg.hit_rate - 1.0).abs() < 1e-9);
    }
}
