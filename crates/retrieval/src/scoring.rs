#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScoreComponents {
    /// α term: skill-level cosine similarity (query vs skill embedding).
    pub l1_semantic: f32,
    /// β term: semantic subunit evidence — the aggregate cosine relevance of the
    /// skill's subunits to the query (NOT lexical token overlap). See issue #172.
    pub subunit_evidence: f32,
    /// γ term: deterministic usage prior.
    pub prior: f32,
    pub community_boost: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScoringWeights {
    pub alpha: f32,
    pub beta: f32,
    pub gamma: f32,
    pub lambda: f32,
}

impl Default for ScoringWeights {
    fn default() -> Self {
        Self {
            alpha: 0.45,
            beta: 0.35,
            gamma: 0.20,
            lambda: 0.25,
        }
    }
}

pub fn score_eq3(components: ScoreComponents, weights: ScoringWeights) -> f32 {
    let base = weights.alpha * components.l1_semantic
        + weights.beta * components.subunit_evidence
        + weights.gamma * components.prior;

    base * (1.0 + weights.lambda * components.community_boost)
}

/// Inputs for computing the deterministic usage prior at graph-load time.
///
/// Populated from a single batched `UsageSampleStore::recent_usage` query in
/// `mcp-server`; kept in `retrieval` so callers don't need to restate the
/// formula's required inputs. Pure data — no persistence logic here.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UsagePriorInputs {
    /// Total skill-usage row count (full history).
    pub usage_count: u32,
    /// Days since the most recent usage, derived server-side from DB `now()`.
    /// Use `u32::MAX` (≈ ~1.17 billion days) to represent "never used" so the
    /// decay term drives the prior to zero.
    pub age_days: u32,
}

/// Computes the deterministic usage prior for a single skill.
///
/// Formula (V1.5 fixed, no adaptive tuning):
///   `min(ln(1 + usage_count) · e^(−age_days / 30), 0.15)`
///
/// - 30-day time constant ≈ 21-day half-life so recent usage weights heavily.
/// - Clamp at 0.15 keeps the prior ≤ ~3% of the final EQ-3 score (gamma=0.20),
///   so it nudges tied skills without overriding relevance.
/// - `usage_count == 0` ⇒ 0.0 (honest cold-start; does not bias unseen skills).
///
/// Composed additively under the existing `gamma` weight in `score_eq3`.
/// The coefficients are sealed constants — write-back or runtime tuning is
/// explicitly out of scope for V1.5.
#[inline]
pub fn usage_prior(usage_count: u32, age_days: u32) -> f32 {
    if usage_count == 0 {
        return 0.0;
    }
    let raw = (1.0 + usage_count as f32).ln() * (-((age_days as f32) / 30.0)).exp();
    raw.min(0.15)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_eq3_matches_contract_formula() {
        let components = ScoreComponents {
            l1_semantic: 0.8,
            subunit_evidence: 0.6,
            prior: 0.4,
            community_boost: 0.3,
        };
        let weights = ScoringWeights::default();

        let expected = (weights.alpha * 0.8 + weights.beta * 0.6 + weights.gamma * 0.4)
            * (1.0 + weights.lambda * 0.3);

        let actual = score_eq3(components, weights);
        assert!((actual - expected).abs() < 1e-6);
    }

    #[test]
    fn usage_prior_returns_zero_for_cold_start() {
        assert_eq!(usage_prior(0, 0), 0.0);
        assert_eq!(usage_prior(0, 100), 0.0);
    }

    #[test]
    fn usage_prior_is_clamped_at_0_15_for_high_usage_count() {
        // ln(1+1000) · e^0 = ln(1001) ≈ 6.91 — should clamp to 0.15.
        let prior = usage_prior(1000, 0);
        assert!((prior - 0.15).abs() < 1e-6, "got {prior}");
    }

    #[test]
    fn usage_prior_decays_with_age() {
        let fresh = usage_prior(5, 0);
        let stale = usage_prior(5, 90);
        assert!(
            fresh > stale,
            "fresh={fresh} should exceed stale={stale} for same usage_count"
        );
    }

    #[test]
    fn usage_prior_matches_contract_formula() {
        // usage_count=1, age_days=120 → ln(2) · e^(−4) ≈ 0.6931 · 0.01832 ≈ 0.01270
        // This stays well below the 0.15 clamp so we can verify the raw formula.
        let expected = (2.0_f32).ln() * (-(120.0_f32 / 30.0)).exp();
        let actual = usage_prior(1, 120);
        assert!(
            (actual - expected).abs() < 1e-6,
            "got {actual}, expected {expected}"
        );
        // Sanity check: result is positive and below clamp.
        assert!(actual > 0.0 && actual < 0.15, "got {actual}");
    }
}
