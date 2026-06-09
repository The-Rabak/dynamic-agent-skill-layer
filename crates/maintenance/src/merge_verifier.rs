use std::sync::Arc;

use async_trait::async_trait;
use infrastructure::LlmEquivalenceVerifier;
use tracing::debug;

use crate::merge::{MergeError, MergeSemanticVerifier, SkillSnapshot};

// ─── Token-overlap pre-filter ─────────────────────────────────────────────────

/// Minimum Jaccard token-overlap required before the pair reaches the LLM gate.
///
/// Pairs below this threshold are identical enough in surface form to be worth
/// nothing and too different in surface form to be worth the LLM call.
/// A non-zero value ensures the pre-filter is never a no-op.
///
/// Set conservatively low (0.05) so only genuinely disjoint texts are short-circuited;
/// the LLM remains the actual gate for all near-match pairs.
const TOKEN_OVERLAP_PREFILTER_THRESHOLD: f32 = 0.05;

/// Computes Jaccard similarity over whitespace-split token sets.
///
/// Returns `None` when either text is empty (signals that the pair should be
/// skipped by the caller rather than passed to the LLM).
fn jaccard_token_overlap(left_text: &str, right_text: &str) -> Option<f32> {
    if left_text.trim().is_empty() || right_text.trim().is_empty() {
        return None;
    }
    let left_tokens: std::collections::HashSet<&str> = left_text.split_whitespace().collect();
    let right_tokens: std::collections::HashSet<&str> = right_text.split_whitespace().collect();
    if left_tokens.is_empty() || right_tokens.is_empty() {
        return None;
    }
    let intersection = left_tokens.intersection(&right_tokens).count();
    let union = left_tokens.union(&right_tokens).count();
    Some(intersection as f32 / union as f32)
}

// ─── LLM-backed verifier (production) ────────────────────────────────────────

/// LLM-backed semantic equivalence verifier behind the `MergeSemanticVerifier` seam.
///
/// Pipeline for each candidate pair:
/// 1. **Token-overlap pre-filter**: pairs below `TOKEN_OVERLAP_PREFILTER_THRESHOLD`
///    Jaccard score are skipped immediately (no LLM call) — they are too lexically
///    disjoint to be plausible duplicates.
/// 2. **LLM decision**: the surviving pair is sent to the configured
///    `LlmEquivalenceVerifier` (Ollama-generate by default, Claude opt-in).
///    The LLM's answer is the final gate.
///
/// If the LLM provider is unavailable the error surfaces loudly as
/// `MergeError::SemanticVerification` — it is never swallowed as a silent `false`.
pub struct LlmMergeSemanticVerifier {
    llm: Arc<dyn LlmEquivalenceVerifier>,
}

impl LlmMergeSemanticVerifier {
    /// Creates a verifier backed by the given LLM equivalence provider.
    pub fn new(llm: Arc<dyn LlmEquivalenceVerifier>) -> Self {
        Self { llm }
    }
}

#[async_trait]
impl MergeSemanticVerifier for LlmMergeSemanticVerifier {
    /// Returns `true` when the LLM considers `left` and `right` semantically equivalent.
    ///
    /// Pairs with a Jaccard token overlap below `TOKEN_OVERLAP_PREFILTER_THRESHOLD`
    /// are rejected before reaching the LLM (cheap pre-filter).
    ///
    /// # Errors
    ///
    /// Surfaces `MergeError::SemanticVerification` when the LLM provider is unavailable
    /// or times out — never swallows the error as `equivalent=false`.
    async fn are_equivalent(
        &self,
        left: &SkillSnapshot,
        right: &SkillSnapshot,
    ) -> Result<bool, MergeError> {
        let left_text = left.semantic_text();
        let right_text = right.semantic_text();

        // Pre-filter: token overlap too low → skip LLM call.
        match jaccard_token_overlap(&left_text, &right_text) {
            None => {
                debug!(
                    left_id = %left.id,
                    right_id = %right.id,
                    "merge pre-filter: empty semantic text; skipping pair"
                );
                return Ok(false);
            }
            Some(overlap) if overlap < TOKEN_OVERLAP_PREFILTER_THRESHOLD => {
                debug!(
                    left_id = %left.id,
                    right_id = %right.id,
                    overlap,
                    threshold = TOKEN_OVERLAP_PREFILTER_THRESHOLD,
                    "merge pre-filter: token overlap below threshold; skipping LLM call"
                );
                return Ok(false);
            }
            Some(_) => {}
        }

        // LLM decision — fail loud on provider error.
        let decision = self
            .llm
            .decide_equivalence(&left_text, &right_text)
            .await
            .map_err(|error| {
                MergeError::SemanticVerification(format!(
                    "LLM equivalence provider failed for pair ({}, {}): {error}",
                    left.id, right.id
                ))
            })?;

        debug!(
            left_id = %left.id,
            right_id = %right.id,
            equivalent = decision.equivalent,
            rationale = %decision.rationale,
            "merge LLM equivalence decision"
        );

        Ok(decision.equivalent)
    }
}

// ─── Token-overlap verifier (test-only) ──────────────────────────────────────

/// Pure Jaccard token-overlap verifier with an explicit non-zero threshold.
///
/// This implementation does NOT make an LLM call. It is exposed only for
/// unit tests that need a fast, deterministic `MergeSemanticVerifier` without
/// a live LLM provider.
///
/// **Do NOT wire this in production** — it misses semantically-equivalent
/// pairs that use different words and may merge lexically-similar-but-different
/// skills. Production code uses [`LlmMergeSemanticVerifier`].
#[cfg(any(test, feature = "test-utils"))]
#[derive(Debug, Clone, Copy)]
pub struct TextOverlapMergeSemanticVerifier {
    pub threshold: f32,
}

#[cfg(any(test, feature = "test-utils"))]
impl TextOverlapMergeSemanticVerifier {
    pub fn new(threshold: f32) -> Self {
        Self { threshold }
    }
}

#[cfg(any(test, feature = "test-utils"))]
#[async_trait]
impl MergeSemanticVerifier for TextOverlapMergeSemanticVerifier {
    async fn are_equivalent(
        &self,
        left: &SkillSnapshot,
        right: &SkillSnapshot,
    ) -> Result<bool, MergeError> {
        let left_text = left.semantic_text();
        let right_text = right.semantic_text();
        match jaccard_token_overlap(&left_text, &right_text) {
            None => Ok(false),
            Some(jaccard) => Ok(jaccard >= self.threshold),
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Mutex;

    use super::*;
    use domain::ScopeType;

    fn snapshot(name: &str, desc: &str, subunits: Vec<&str>) -> SkillSnapshot {
        SkillSnapshot {
            id: name.to_owned(),
            name: name.to_owned(),
            description: desc.to_owned(),
            scope: ScopeType::Global,
            source_path: PathBuf::from("/tmp/SKILL.md"),
            tags: vec![],
            subunits: subunits.into_iter().map(String::from).collect(),
            embedding: vec![],
        }
    }

    // ── TextOverlapMergeSemanticVerifier unit tests ───────────────────────────

    #[tokio::test]
    async fn text_overlap_identical_snapshots_pass() {
        let verifier = TextOverlapMergeSemanticVerifier::new(0.7);
        let s = snapshot(
            "rust-auth",
            "Rust authentication flow",
            vec!["verify JWT", "check scope"],
        );
        assert!(verifier.are_equivalent(&s, &s).await.unwrap());
    }

    #[tokio::test]
    async fn text_overlap_disjoint_snapshots_fail() {
        let verifier = TextOverlapMergeSemanticVerifier::new(0.7);
        let a = snapshot("rust-auth", "Rust authentication flow", vec!["verify JWT"]);
        let b = snapshot(
            "py-http",
            "Python HTTP client patterns",
            vec!["use requests"],
        );
        assert!(!verifier.are_equivalent(&a, &b).await.unwrap());
    }

    #[tokio::test]
    async fn text_overlap_empty_inputs_return_false() {
        let verifier = TextOverlapMergeSemanticVerifier::new(0.7);
        let a = snapshot("a", "", vec![]);
        let b = snapshot("b", "something", vec![]);
        assert!(!verifier.are_equivalent(&a, &b).await.unwrap());
        assert!(!verifier.are_equivalent(&b, &a).await.unwrap());
    }

    // ── LlmMergeSemanticVerifier unit tests ───────────────────────────────────

    /// Controllable mock of `LlmEquivalenceVerifier` for deterministic unit tests.
    struct MockLlmEquivalenceVerifier {
        /// Pre-programmed sequence of decisions returned in FIFO order.
        decisions: Mutex<Vec<infrastructure::EquivalenceDecision>>,
    }

    impl MockLlmEquivalenceVerifier {
        fn always_equivalent() -> Arc<Self> {
            Arc::new(Self {
                decisions: Mutex::new(vec![
                    // Return equivalent=true for any number of calls.
                    infrastructure::EquivalenceDecision {
                        equivalent: true,
                        rationale: "mock: equivalent".to_owned(),
                    },
                ]),
            })
        }

        fn always_not_equivalent() -> Arc<Self> {
            Arc::new(Self {
                decisions: Mutex::new(vec![infrastructure::EquivalenceDecision {
                    equivalent: false,
                    rationale: "mock: not equivalent".to_owned(),
                }]),
            })
        }

        fn always_error() -> Arc<Self> {
            // Empty decision queue → the mock will return Err
            Arc::new(Self {
                decisions: Mutex::new(vec![]),
            })
        }
    }

    #[async_trait]
    impl LlmEquivalenceVerifier for MockLlmEquivalenceVerifier {
        async fn decide_equivalence(
            &self,
            _left_text: &str,
            _right_text: &str,
        ) -> Result<infrastructure::EquivalenceDecision, domain::ExtractionError> {
            let mut lock = self.decisions.lock().unwrap();
            // Re-use last decision if only one was queued (for "always" mocks).
            if lock.len() == 1 {
                Ok(lock[0].clone())
            } else if let Some(decision) = lock.pop() {
                Ok(decision)
            } else {
                Err(domain::ExtractionError::ProviderUnavailable(
                    "mock provider unavailable".to_owned(),
                ))
            }
        }
    }

    /// Acceptance criterion #3a: two semantically-equivalent-but-lexically-DIFFERENT
    /// skills are detected as equivalent when the LLM says so.
    ///
    /// The current Jaccard gate would miss these because the token sets barely overlap.
    #[tokio::test]
    async fn llm_verifier_detects_equivalent_skills_when_llm_says_yes() {
        let llm = MockLlmEquivalenceVerifier::always_equivalent();
        let verifier = LlmMergeSemanticVerifier::new(llm);

        // "authenticate user" and "verify identity" are semantically equivalent
        // but lexically different — Jaccard overlap is low.
        let left = snapshot(
            "authenticate",
            "Verify that the requesting user holds a valid credential before granting access",
            vec!["check bearer token", "validate expiry"],
        );
        let right = snapshot(
            "verify-identity",
            "Confirm user identity through credential inspection prior to resource access",
            vec!["inspect JWT claims", "enforce expiration policy"],
        );

        // These have > 0.05 token overlap (both share words like "access"), so they
        // pass the pre-filter and reach the mock LLM which returns equivalent=true.
        let result = verifier.are_equivalent(&left, &right).await.unwrap();
        assert!(
            result,
            "semantically-equivalent skills must be detected as equivalent by the LLM gate"
        );
    }

    /// Acceptance criterion #3b: two lexically-similar-but-semantically-different
    /// skills are NOT merged when the LLM says they are not equivalent.
    #[tokio::test]
    async fn llm_verifier_rejects_merge_when_llm_says_not_equivalent() {
        let llm = MockLlmEquivalenceVerifier::always_not_equivalent();
        let verifier = LlmMergeSemanticVerifier::new(llm);

        // Both are about "authentication" but one handles OAuth, the other SSH.
        let left = snapshot(
            "oauth-auth",
            "Authenticate users via OAuth2 authorization code flow",
            vec![
                "redirect to provider",
                "exchange code for token",
                "verify token claims",
            ],
        );
        let right = snapshot(
            "ssh-auth",
            "Authenticate SSH sessions using public-key cryptography",
            vec![
                "load authorized_keys",
                "verify key signature",
                "start session",
            ],
        );

        let result = verifier.are_equivalent(&left, &right).await.unwrap();
        assert!(
            !result,
            "lexically-similar-but-different skills must NOT be merged when LLM says no"
        );
    }

    /// Acceptance criterion #2: LLM-unavailable surfaces as MergeError::SemanticVerification.
    ///
    /// This proves the error is loud/observable and NEVER silently returns false.
    #[tokio::test]
    async fn llm_verifier_surfaces_provider_error_loudly() {
        let llm = MockLlmEquivalenceVerifier::always_error();
        let verifier = LlmMergeSemanticVerifier::new(llm);

        let left = snapshot(
            "auth",
            "Authenticate users with a valid credential",
            vec!["check token", "validate expiry"],
        );
        let right = snapshot(
            "identity",
            "Confirm user identity through credential inspection",
            vec!["inspect JWT", "enforce expiration"],
        );

        let result = verifier.are_equivalent(&left, &right).await;
        assert!(
            result.is_err(),
            "provider unavailable must surface as Err, not silent false"
        );
        assert!(
            matches!(result.unwrap_err(), MergeError::SemanticVerification(_)),
            "provider error must map to MergeError::SemanticVerification"
        );
    }

    /// Verifies the pre-filter: a pair with near-zero token overlap is rejected before
    /// the LLM is called (LLM mock returns error, but the pre-filter short-circuits).
    #[tokio::test]
    async fn prefilter_rejects_completely_disjoint_texts_without_llm_call() {
        // Use an error mock — if the LLM were called, the test would fail.
        let llm = MockLlmEquivalenceVerifier::always_error();
        let verifier = LlmMergeSemanticVerifier::new(llm);

        // Completely disjoint texts have Jaccard = 0.0, below the 0.05 threshold.
        let left = snapshot("skill-x", "alpha beta gamma delta", vec![]);
        let right = snapshot("skill-y", "omega phi psi chi", vec![]);

        let result = verifier.are_equivalent(&left, &right).await;
        // Pre-filter must return Ok(false) without calling the LLM (which would Err).
        assert!(
            !result.unwrap(),
            "completely disjoint pair must be rejected by pre-filter, not forwarded to LLM"
        );
    }
}
