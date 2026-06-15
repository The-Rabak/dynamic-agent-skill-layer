//! Session-start priming selection: recurrence boost + bounded freshness injection.
//!
//! This module owns the final candidate selection step for `RetrievalIntent::Priming`.
//! It is intentionally a pure function with no async dependencies so it is
//! straightforward to test and compose without touching the orchestrator plumbing.
//!
//! # Selection algorithm
//!
//! Given the fused candidates (already score-descending from RRF), the selector:
//!
//! 1. **Recurrence rerank**: adds `recurrence_weight * prior` to each candidate's score,
//!    then re-sorts. The prior (`≤ 0.15`) is a bounded additive boost so query-specific
//!    relevance stays dominant — a high-prior skill cannot displace a clearly more
//!    relevant one (the boost is at most `0.10 * 0.15 = 0.015` with defaults).
//!
//! 2. **Freshness slot injection**: reserves up to `freshness_slots` positions for the
//!    highest-reranked FRESH candidate(s) (age_days ≤ freshness_window_days) that would
//!    otherwise fall outside the top-N. A fresh candidate already in the top-N consumes
//!    no extra slot. Injection operates over the existing floor-passing pool — it does NOT
//!    add new candidate sources.
//!
//! 3. **Bound**: result length is always ≤ `max_results`; no duplicate indices.
//!
//! # Degenerate cases
//!
//! - `recurrence_weight = 0`, `freshness_slots = 0` → plain top-N by score (no behavior change).
//! - Empty ranked pool → empty result.
//! - `max_results = 0` → empty result.

use crate::fusion::FusedCandidate;

/// Configuration for priming candidate selection.
///
/// All fields carry the conservatively-chosen T12 defaults; they are env-overridable
/// via `RetrievalConfig` so Unit 4 can measure each lever independently.
#[derive(Debug, Clone, Copy)]
pub struct PrimingRankConfig {
    /// Maximum number of candidates to return (bound on result size).
    pub max_results: usize,
    /// Additive weight applied to the usage prior during recurrence reranking.
    /// Default 0.10 — keeps query-relevance dominant (prior ≤ 0.15 → max boost ≈ 0.015).
    pub recurrence_weight: f32,
    /// Number of bottom slots reserved for freshness injection (from the existing pool).
    /// Default 1 — at most one fresh skill displaces the lowest non-fresh slot.
    pub freshness_slots: usize,
    /// Skills whose age_days ≤ this value are considered "fresh" for slot injection.
    /// Default 30 days.
    pub freshness_window_days: u32,
}

/// Selects the bounded SessionStart prime from the fused candidates.
///
/// # Parameters
///
/// - `ranked`: fused candidates in score-descending order (from `weighted_reciprocal_rank_fusion`).
/// - `prior_of`: maps a `skill_index` (index into `ranked`) to the γ usage prior (≤ 0.15).
/// - `age_days_of`: maps a `skill_id` to whole days since creation. `None` = unknown → not fresh.
/// - `cfg`: selection configuration (see [`PrimingRankConfig`]).
///
/// # Returns
///
/// Indices into `ranked` in final inject order, length ≤ `cfg.max_results`.
/// The indices are stable references into the caller's `ranked` slice; the caller
/// is responsible for mapping them back to skills and scores.
///
/// With `recurrence_weight = 0` and `freshness_slots = 0` this is exactly
/// "top max_results by original score" (degenerate / plain top-N).
pub fn select_priming_prime(
    ranked: &[FusedCandidate],
    prior_of: impl Fn(usize) -> f32,
    age_days_of: impl Fn(&str) -> Option<u32>,
    cfg: PrimingRankConfig,
) -> Vec<usize> {
    if ranked.is_empty() || cfg.max_results == 0 {
        return Vec::new();
    }

    // ── Step 1: recurrence rerank ────────────────────────────────────────────
    // Compute reranked scores and sort indices by that score (descending).
    // Ties preserve the original order (stable relative to `ranked` ordering).
    let mut reranked_indices: Vec<usize> = (0..ranked.len()).collect();
    let reranked_score = |idx: usize| -> f32 {
        let base = ranked[idx].score;
        let prior = prior_of(idx);
        base + cfg.recurrence_weight * prior
    };

    reranked_indices.sort_by(|&left, &right| {
        let score_l = reranked_score(left);
        let score_r = reranked_score(right);
        // Descending: higher reranked score first; ties preserve original order.
        score_r.total_cmp(&score_l).then_with(|| left.cmp(&right)) // stable tie-break: earlier = better
    });

    // ── Step 2: freshness slot injection ────────────────────────────────────
    // Collect the top-N candidates (unconstrained by freshness) and the candidates
    // that fall outside the top-N. Among the outside candidates, identify those that
    // are fresh (age ≤ window). Inject up to `freshness_slots` fresh candidates into
    // the reserved tail positions, displacing the lowest-ranked non-fresh slot(s).
    //
    // A fresh candidate already in the top-N consumes no injection slot.
    if cfg.freshness_slots == 0 || cfg.max_results == 0 {
        // Degenerate case: plain top-N, no freshness injection.
        return reranked_indices.into_iter().take(cfg.max_results).collect();
    }

    let top_n: Vec<usize> = reranked_indices
        .iter()
        .copied()
        .take(cfg.max_results)
        .collect();

    // Fresh candidates outside the top-N (still ordered by reranked score).
    let fresh_outside_top_n: Vec<usize> = reranked_indices
        .iter()
        .copied()
        .skip(cfg.max_results)
        .filter(|&idx| {
            let skill_id = &ranked[idx].skill_id;
            age_days_of(skill_id)
                .map(|age| age <= cfg.freshness_window_days)
                .unwrap_or(false) // None = unknown → not fresh
        })
        .take(cfg.freshness_slots)
        .collect();

    if fresh_outside_top_n.is_empty() {
        // No fresh candidates outside top-N: return plain top-N.
        return top_n;
    }

    // Determine how many positions to inject: bounded by freshness_slots AND by how
    // many non-fresh slots exist in top_n to displace (fresh skills already in top-N
    // are never displaced — they earn their slot on merit).
    //
    // Walk top_n from the bottom, marking non-fresh slots as displaced to make room.
    let desired_injection = fresh_outside_top_n.len().min(cfg.freshness_slots);
    let mut displaces_needed = desired_injection;
    let mut displaced_indices = std::collections::HashSet::new();
    for &idx in top_n.iter().rev() {
        if displaces_needed == 0 {
            break;
        }
        let skill_id = &ranked[idx].skill_id;
        let is_fresh = age_days_of(skill_id)
            .map(|age| age <= cfg.freshness_window_days)
            .unwrap_or(false);
        if !is_fresh {
            displaced_indices.insert(idx);
            displaces_needed -= 1;
        }
    }
    // Only inject as many as we could displace (all-fresh top-N → no room to inject).
    let actual_injection = desired_injection - displaces_needed;

    // Build kept: top_n slots not in the displaced set.
    let mut kept_top_n: Vec<usize> = Vec::with_capacity(cfg.max_results);
    for &idx in &top_n {
        if !displaced_indices.contains(&idx) {
            kept_top_n.push(idx);
        }
    }

    // Append the (up to actual_injection) fresh slots.
    let injected: Vec<usize> = fresh_outside_top_n
        .into_iter()
        .take(actual_injection)
        .collect();

    let mut result = kept_top_n;
    result.extend(injected);

    // Safety invariant: no duplicates (top_n_set ∩ injected = ∅ by construction).
    debug_assert!(
        result.len() <= cfg.max_results,
        "select_priming_prime: result length {} exceeds max_results {}",
        result.len(),
        cfg.max_results,
    );
    debug_assert_eq!(
        result.len(),
        result
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        "select_priming_prime: result contains duplicate indices"
    );

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::ScopeType;

    /// Builds a `FusedCandidate` for testing with minimal fields set.
    fn candidate(skill_index: usize, score: f32) -> FusedCandidate {
        FusedCandidate {
            skill_index,
            skill_id: format!("skill-{skill_index}"),
            matched_scope: ScopeType::Global,
            score,
            semantic_score: score,
            lexical_score: 0.0,
            subunit_evidence: 0.0,
            embedding: vec![1.0, 0.0],
            highlights: Vec::new(),
        }
    }

    fn no_prior(_idx: usize) -> f32 {
        0.0
    }

    fn no_age(_id: &str) -> Option<u32> {
        None
    }

    /// Degenerate: weight=0 + slots=0 → returns the top max_results by original score.
    #[test]
    fn degenerate_no_recurrence_no_freshness_returns_top_n_by_score() {
        let ranked = vec![
            candidate(0, 0.9),
            candidate(1, 0.8),
            candidate(2, 0.7),
            candidate(3, 0.5),
        ];
        let cfg = PrimingRankConfig {
            max_results: 3,
            recurrence_weight: 0.0,
            freshness_slots: 0,
            freshness_window_days: 30,
        };

        let selected = select_priming_prime(&ranked, no_prior, no_age, cfg);

        assert_eq!(
            selected,
            vec![0, 1, 2],
            "should return top 3 by original score"
        );
    }

    /// Recurrence boost: a high-prior candidate at rank 2 (score 0.79) gets
    /// promoted above rank 1 (score 0.80) when weight * prior exceeds the gap.
    #[test]
    fn recurrence_boost_promotes_high_prior_candidate() {
        // Scores: rank0=0.90, rank1=0.80, rank2=0.79 (close gap), rank3=0.50
        // Prior for rank2 = 0.15 (max), weight=0.10 → boost=0.015
        // Reranked rank2 score = 0.79 + 0.015 = 0.805 > rank1 0.80 → promoted
        let ranked = vec![
            candidate(0, 0.90),
            candidate(1, 0.80),
            candidate(2, 0.79),
            candidate(3, 0.50),
        ];
        let cfg = PrimingRankConfig {
            max_results: 3,
            recurrence_weight: 0.10,
            freshness_slots: 0,
            freshness_window_days: 30,
        };

        // prior_of: skill index 2 has prior=0.15; others have 0.0
        let prior_of = |idx: usize| if idx == 2 { 0.15_f32 } else { 0.0 };

        let selected = select_priming_prime(&ranked, prior_of, no_age, cfg);

        // After reranking: idx0=0.90, idx2=0.805, idx1=0.80 → order: [0, 2, 1]
        assert_eq!(selected.len(), 3, "must return exactly max_results");
        assert_eq!(selected[0], 0, "highest score stays at top");
        assert_eq!(
            selected[1], 2,
            "high-prior rank2 promoted above rank1 (0.805 > 0.80)"
        );
        assert_eq!(selected[2], 1, "rank1 demoted to 3rd after boost");
    }

    /// Freshness slot: a FRESH candidate ranked just outside top-N is injected
    /// into the last slot, displacing the lowest non-fresh candidate.
    #[test]
    fn freshness_slot_injects_fresh_candidate_outside_top_n() {
        // 4 candidates, max_results=3, freshness_slots=1
        // idx 0,1,2 are in top-3 by score; idx3 = fresh (age=5 ≤ 30 window)
        // All in top-3 are non-fresh (age=None → unknown)
        // Expected: idx0, idx1 kept; idx2 (lowest non-fresh) displaced; idx3 injected.
        let ranked = vec![
            candidate(0, 0.90),
            candidate(1, 0.80),
            candidate(2, 0.70),
            candidate(3, 0.60), // fresh, outside top-3
        ];
        let cfg = PrimingRankConfig {
            max_results: 3,
            recurrence_weight: 0.0,
            freshness_slots: 1,
            freshness_window_days: 30,
        };
        let age_days_of = |skill_id: &str| -> Option<u32> {
            if skill_id == "skill-3" { Some(5) } else { None } // only skill-3 is fresh
        };

        let selected = select_priming_prime(&ranked, no_prior, age_days_of, cfg);

        assert_eq!(selected.len(), 3, "result must be bounded to max_results");
        assert!(
            selected.contains(&0),
            "skill-0 (score 0.90) must stay in result"
        );
        assert!(
            selected.contains(&1),
            "skill-1 (score 0.80) must stay in result"
        );
        assert!(
            selected.contains(&3),
            "skill-3 (fresh, outside top-3) must be injected"
        );
        assert!(
            !selected.contains(&2),
            "skill-2 (lowest non-fresh) must be displaced by fresh injection"
        );
    }

    /// A candidate with unknown age (None) is never treated as fresh.
    #[test]
    fn unknown_age_candidate_is_not_injected() {
        let ranked = vec![
            candidate(0, 0.90),
            candidate(1, 0.80),
            candidate(2, 0.70),
            candidate(3, 0.60), // age=None → unknown → not fresh
        ];
        let cfg = PrimingRankConfig {
            max_results: 3,
            recurrence_weight: 0.0,
            freshness_slots: 1,
            freshness_window_days: 30,
        };
        // No skill has a known age → no injection should occur.

        let selected = select_priming_prime(&ranked, no_prior, no_age, cfg);

        assert_eq!(
            selected,
            vec![0, 1, 2],
            "no fresh candidates → plain top-N returned"
        );
    }

    /// freshness_slots=0 → no injection even when fresh candidates exist outside top-N.
    #[test]
    fn freshness_slots_zero_means_no_injection() {
        let ranked = vec![
            candidate(0, 0.90),
            candidate(1, 0.80),
            candidate(2, 0.70),
            candidate(3, 0.60), // would be fresh, but slots=0 prevents injection
        ];
        let cfg = PrimingRankConfig {
            max_results: 3,
            recurrence_weight: 0.0,
            freshness_slots: 0,
            freshness_window_days: 30,
        };
        let age_days_of = |id: &str| if id == "skill-3" { Some(5) } else { None };

        let selected = select_priming_prime(&ranked, no_prior, age_days_of, cfg);

        assert_eq!(
            selected,
            vec![0, 1, 2],
            "freshness_slots=0 must prevent injection regardless of fresh candidates"
        );
    }

    /// Result length ≤ max_results regardless of input size.
    #[test]
    fn result_length_bounded_by_max_results() {
        let ranked: Vec<_> = (0..10)
            .map(|i| candidate(i, 1.0 - i as f32 * 0.05))
            .collect();
        let cfg = PrimingRankConfig {
            max_results: 4,
            recurrence_weight: 0.10,
            freshness_slots: 2,
            freshness_window_days: 30,
        };
        let age_days_of = |id: &str| -> Option<u32> {
            // skills 7, 8, 9 are fresh
            let idx: usize = id.trim_start_matches("skill-").parse().unwrap_or(999);
            if idx >= 7 { Some(10) } else { None }
        };

        let selected = select_priming_prime(&ranked, no_prior, age_days_of, cfg);

        assert!(
            selected.len() <= 4,
            "result must not exceed max_results=4; got {}",
            selected.len()
        );
    }

    /// No duplicate indices in result.
    #[test]
    fn result_contains_no_duplicate_indices() {
        let ranked = vec![
            candidate(0, 0.90),
            candidate(1, 0.80),
            candidate(2, 0.70),
            candidate(3, 0.60),
            candidate(4, 0.50),
        ];
        let cfg = PrimingRankConfig {
            max_results: 3,
            recurrence_weight: 0.10,
            freshness_slots: 2,
            freshness_window_days: 30,
        };
        let age_days_of = |id: &str| -> Option<u32> {
            let idx: usize = id.trim_start_matches("skill-").parse().unwrap_or(999);
            if idx >= 3 { Some(7) } else { None }
        };

        let selected = select_priming_prime(&ranked, no_prior, age_days_of, cfg);

        let unique_count = selected
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len();
        assert_eq!(
            unique_count,
            selected.len(),
            "no duplicate indices in result; got {:?}",
            selected
        );
    }

    /// Empty ranked pool → empty result.
    #[test]
    fn empty_pool_returns_empty() {
        let cfg = PrimingRankConfig {
            max_results: 3,
            recurrence_weight: 0.10,
            freshness_slots: 1,
            freshness_window_days: 30,
        };
        let selected = select_priming_prime(&[], no_prior, no_age, cfg);
        assert!(selected.is_empty(), "empty pool must return empty result");
    }

    /// max_results=0 → empty result.
    #[test]
    fn max_results_zero_returns_empty() {
        let ranked = vec![candidate(0, 0.9), candidate(1, 0.8)];
        let cfg = PrimingRankConfig {
            max_results: 0,
            recurrence_weight: 0.10,
            freshness_slots: 1,
            freshness_window_days: 30,
        };
        let selected = select_priming_prime(&ranked, no_prior, no_age, cfg);
        assert!(
            selected.is_empty(),
            "max_results=0 must return empty result"
        );
    }

    /// Fresh candidate already in top-N consumes no extra injection slot.
    #[test]
    fn fresh_candidate_already_in_top_n_consumes_no_injection_slot() {
        // skill-0 is fresh and already in top-3; skill-3 is also fresh but outside top-3.
        // With freshness_slots=1 the slot should NOT be wasted on skill-0 (already in).
        // skill-3 (fresh, outside) should be injected → displaces skill-2 (non-fresh, lowest).
        let ranked = vec![
            candidate(0, 0.90), // fresh, in top-3
            candidate(1, 0.80), // not fresh
            candidate(2, 0.70), // not fresh, lowest non-fresh in top-3
            candidate(3, 0.60), // fresh, outside top-3
        ];
        let cfg = PrimingRankConfig {
            max_results: 3,
            recurrence_weight: 0.0,
            freshness_slots: 1,
            freshness_window_days: 30,
        };
        let age_days_of = |id: &str| -> Option<u32> {
            match id {
                "skill-0" | "skill-3" => Some(5),
                _ => None,
            }
        };

        let selected = select_priming_prime(&ranked, no_prior, age_days_of, cfg);

        assert_eq!(selected.len(), 3, "result bounded to max_results=3");
        assert!(
            selected.contains(&0),
            "fresh skill-0 must stay in result (already in top-N)"
        );
        assert!(selected.contains(&1), "skill-1 must stay in result");
        assert!(
            selected.contains(&3),
            "fresh skill-3 (outside top-3) must be injected"
        );
        assert!(
            !selected.contains(&2),
            "skill-2 (lowest non-fresh) must be displaced"
        );
    }
}
