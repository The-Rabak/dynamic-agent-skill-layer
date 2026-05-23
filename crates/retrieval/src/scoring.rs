#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScoreComponents {
    pub l1_semantic: f32,
    pub l0_lexical: f32,
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
        + weights.beta * components.l0_lexical
        + weights.gamma * components.prior;

    base * (1.0 + weights.lambda * components.community_boost)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_eq3_matches_contract_formula() {
        let components = ScoreComponents {
            l1_semantic: 0.8,
            l0_lexical: 0.6,
            prior: 0.4,
            community_boost: 0.3,
        };
        let weights = ScoringWeights::default();

        let expected = (weights.alpha * 0.8 + weights.beta * 0.6 + weights.gamma * 0.4)
            * (1.0 + weights.lambda * 0.3);

        let actual = score_eq3(components, weights);
        assert!((actual - expected).abs() < 1e-6);
    }
}
