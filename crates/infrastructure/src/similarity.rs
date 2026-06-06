//! Shared cosine-similarity computation used by both the maintenance merge pass
//! and the session-extractor reduce step.
//!
//! ## Why shared
//!
//! Before this module, `cosine_similarity` lived only in `maintenance/src/merge.rs`
//! (crate-private). The extraction-scaling epic's orchestrator (`session-extractor`)
//! also needs cosine pairing for its intra-session reduce step. Rather than
//! duplicate the logic or accept a crate-level coupling in the wrong direction,
//! the pure function is lifted here — into `infrastructure`, which both crates
//! already depend on — so there is exactly ONE cosine implementation in the repo.
//!
//! ## Error type
//!
//! Errors are represented as [`CosineSimilarityError`] rather than borrowing
//! `MergeError`, so callers in different contexts can convert to their own error
//! type without a cross-crate dependency on maintenance.

use thiserror::Error;

/// Failure modes of [`cosine_similarity`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CosineSimilarityError {
    /// The two vectors have different lengths; cosine similarity is undefined.
    #[error("embedding dimension mismatch: left={left_dimension}, right={right_dimension}")]
    DimensionMismatch {
        left_dimension: usize,
        right_dimension: usize,
    },
    /// At least one vector has a zero L₂-norm; cosine similarity is undefined.
    #[error("cannot compare zero-magnitude embedding vectors")]
    ZeroMagnitude,
}

/// Computes the cosine similarity between two equal-length embedding vectors.
///
/// Returns a value in `[-1.0, 1.0]`. Returns `Err` when the vectors have
/// different lengths or when either vector has a zero L₂-norm (because cosine
/// similarity is undefined in both cases).
///
/// # Panics
///
/// Never panics. All arithmetic is bounds-checked.
pub fn cosine_similarity(left: &[f32], right: &[f32]) -> Result<f32, CosineSimilarityError> {
    if left.len() != right.len() {
        return Err(CosineSimilarityError::DimensionMismatch {
            left_dimension: left.len(),
            right_dimension: right.len(),
        });
    }
    let mut dot_product: f32 = 0.0;
    let mut left_norm: f32 = 0.0;
    let mut right_norm: f32 = 0.0;
    for (&lv, &rv) in left.iter().zip(right.iter()) {
        dot_product += lv * rv;
        left_norm += lv * lv;
        right_norm += rv * rv;
    }
    if left_norm == 0.0 || right_norm == 0.0 {
        return Err(CosineSimilarityError::ZeroMagnitude);
    }
    Ok(dot_product / (left_norm.sqrt() * right_norm.sqrt()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_unit_vectors_return_one() {
        let v = vec![1.0_f32, 0.0, 0.0];
        let result = cosine_similarity(&v, &v).expect("identical vectors must succeed");
        assert!(
            (result - 1.0).abs() < 1e-6,
            "identical vectors must have cosine similarity 1.0; got {result}"
        );
    }

    #[test]
    fn orthogonal_vectors_return_zero() {
        let a = vec![1.0_f32, 0.0];
        let b = vec![0.0_f32, 1.0];
        let result = cosine_similarity(&a, &b).expect("orthogonal vectors must succeed");
        assert!(
            result.abs() < 1e-6,
            "orthogonal vectors must have cosine similarity ~0; got {result}"
        );
    }

    #[test]
    fn opposite_vectors_return_negative_one() {
        let a = vec![1.0_f32, 0.0];
        let b = vec![-1.0_f32, 0.0];
        let result = cosine_similarity(&a, &b).expect("opposite vectors must succeed");
        assert!(
            (result + 1.0).abs() < 1e-6,
            "opposite vectors must have cosine similarity -1.0; got {result}"
        );
    }

    #[test]
    fn dimension_mismatch_returns_error() {
        let a = vec![1.0_f32, 0.0];
        let b = vec![1.0_f32, 0.0, 0.0];
        let error = cosine_similarity(&a, &b).expect_err("dimension mismatch must fail");
        assert!(
            matches!(
                error,
                CosineSimilarityError::DimensionMismatch {
                    left_dimension: 2,
                    right_dimension: 3
                }
            ),
            "unexpected error variant: {error:?}"
        );
    }

    #[test]
    fn zero_magnitude_vector_returns_error() {
        let zero = vec![0.0_f32, 0.0];
        let other = vec![1.0_f32, 0.0];
        let error = cosine_similarity(&zero, &other).expect_err("zero-magnitude must fail");
        assert!(
            matches!(error, CosineSimilarityError::ZeroMagnitude),
            "expected ZeroMagnitude, got: {error:?}"
        );
    }

    #[test]
    fn near_duplicate_vectors_score_close_to_one() {
        // Two vectors that point nearly the same direction must score > 0.99.
        let a = vec![0.6_f32, 0.8, 0.0];
        let b = vec![0.601_f32, 0.799, 0.001];
        let result = cosine_similarity(&a, &b).expect("near-duplicate must succeed");
        assert!(
            result > 0.99,
            "near-duplicate vectors must score > 0.99; got {result}"
        );
    }
}
