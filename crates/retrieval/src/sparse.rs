/// BM25-style sparse vector construction for Qdrant hybrid search.
///
/// # Why no IDF at write time
///
/// Qdrant collections created with `"modifier": "idf"` on the sparse vector slot
/// apply IDF re-weighting at query time on the server side. That makes query-time
/// IDF correct across the live corpus even when the corpus changes between rebuilds.
/// Therefore, document-side weights only need the BM25 TF-saturation component:
///
///   `w(t, d) = tf * (k1 + 1) / (tf + k1 * (1 - b + b * dl / avgdl))`
///
/// with k1 = 1.2, b = 0.75 (same constants as `Bm25Index`).
///
/// # Why query vectors use weight 1.0
///
/// Query terms appear at most once in a short query, making TF saturation
/// irrelevant. Qdrant's IDF modifier already weights them server-side. Sending 1.0
/// per distinct term is therefore the canonical convention for BM25-style sparse
/// queries with IDF modifiers.
///
/// # Term-to-index mapping
///
/// Term IDs are derived by `term_to_sparse_index` (FNV-1a truncated to u32).
/// The hash is computed identically at write time (here) and at query time
/// (`query_sparse_vector`), so the same term always maps to the same sparse index
/// regardless of which corpus was loaded.
use std::collections::HashMap;

use crate::bm25::Bm25Index;

/// BM25 TF-saturation constants — must match `Bm25Index`.
const K1: f32 = 1.2;
const B: f32 = 0.75;

/// Maps a lowercase token to a stable u32 sparse-vector index using FNV-1a.
///
/// FNV-1a (32-bit) is chosen for its fast computation, zero dependencies, and
/// deterministic output that never changes between Rust toolchain versions or
/// platforms — unlike `std::hash::DefaultHasher`, which is explicitly
/// non-deterministic across runs and versions (see the standard library docs).
///
/// Collisions (two distinct terms mapping to the same index) are harmless for
/// BM25-style retrieval: a collision merges two terms' weights additively into one
/// bucket, which slightly inflates the score for documents that happen to contain
/// one of the colliding terms. Given a 32-bit hash space (~4 billion buckets) and a
/// realistic vocabulary of ≤100 000 terms, the collision probability per term pair
/// is ~1 in 43 000 — negligibly rare in practice.
///
/// The caller is responsible for lowercasing the term before calling this function.
pub fn term_to_sparse_index(term: &str) -> u32 {
    // FNV-1a 32-bit: offset_basis = 2166136261, prime = 16777619.
    const FNV_OFFSET: u32 = 2_166_136_261;
    const FNV_PRIME: u32 = 16_777_619;

    let mut hash = FNV_OFFSET;
    for byte in term.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Tokenizes text the same way as `Bm25Index::build` and `Bm25Index::score`:
/// split on non-alphanumeric characters, lowercase, filter empty tokens.
///
/// This function is intentionally private — callers go through
/// `build_skill_sparse_vectors` and `query_sparse_vector`.
fn tokenize(text: &str) -> Vec<String> {
    text.split(|ch: char| !ch.is_alphanumeric())
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty())
        .collect()
}

/// Builds per-skill BM25 TF-saturation sparse vectors from the skill corpus.
///
/// Takes a slice of `(skill_index, lexical_doc_text)` pairs (the same format
/// passed to `Bm25Index::build`) and a pre-built `Bm25Index` for the corpus
/// statistics (average document length, per-doc lengths).
///
/// Returns one `(indices, values)` pair per skill, in the same order as the
/// input slice. Each pair is suitable for passing to
/// `QdrantAdapter::upsert_hybrid_point` after wrapping in a `SparseVector`.
///
/// Terms with a computed weight of exactly 0.0 are omitted (Qdrant sparse
/// vectors must only carry non-zero components).
///
/// # Panics
///
/// Does not panic. Skills with no tokens produce an empty `(vec![], vec![])`.
pub fn build_skill_sparse_vectors(
    docs: &[(usize, String)],
    bm25_index: &Bm25Index,
) -> Vec<(Vec<u32>, Vec<f32>)> {
    let avgdl = bm25_index.avg_doc_length();
    let doc_lengths = bm25_index.doc_lengths_map();

    docs.iter()
        .map(|(skill_index, text)| {
            let tokens = tokenize(text);
            let dl = doc_lengths
                .get(skill_index)
                .copied()
                .unwrap_or(tokens.len()) as f32;
            let length_norm = 1.0 - B + B * (dl / avgdl.max(1.0));

            // Accumulate term frequencies for this document.
            let mut tf_map: HashMap<String, usize> = HashMap::new();
            for token in &tokens {
                *tf_map.entry(token.clone()).or_insert(0) += 1;
            }

            // Convert each term to its BM25 TF-saturation weight.
            let mut index_weight: HashMap<u32, f32> = HashMap::new();
            for (term, &tf) in &tf_map {
                let tf_f = tf as f32;
                let weight = tf_f * (K1 + 1.0) / (tf_f + K1 * length_norm);
                if weight > 0.0 {
                    // Additive merge on hash collision (harmless for BM25 scoring).
                    *index_weight
                        .entry(term_to_sparse_index(term))
                        .or_insert(0.0) += weight;
                }
            }

            let mut indices: Vec<u32> = Vec::with_capacity(index_weight.len());
            let mut values: Vec<f32> = Vec::with_capacity(index_weight.len());
            for (idx, val) in index_weight {
                indices.push(idx);
                values.push(val);
            }
            (indices, values)
        })
        .collect()
}

/// Builds a query sparse vector for a natural-language query string.
///
/// Each distinct lowercased token contributes weight `1.0` at its FNV-1a index.
/// IDF scaling is applied server-side by Qdrant's `"modifier": "idf"` on the
/// collection's sparse vector slot — there is nothing further to compute here.
///
/// Duplicate query terms are de-duplicated: repeated occurrences of the same token
/// do not accumulate; the first occurrence establishes weight `1.0` and subsequent
/// occurrences of the same token are ignored. This matches the standard BM25-query
/// convention where TF saturation applies only on the document side and query terms
/// are unit-weighted.
///
/// Genuine hash collisions (two *distinct* tokens that happen to share the same FNV-1a
/// index) also resolve to weight `1.0` at the shared bucket — the `or_insert` is
/// idempotent and does not accumulate across collisions either.
///
/// Returns `(indices, values)` in unspecified order. Both slices are the same
/// length. An empty query produces empty slices.
pub fn query_sparse_vector(query: &str) -> (Vec<u32>, Vec<f32>) {
    let tokens = tokenize(query);
    let mut index_weight: HashMap<u32, f32> = HashMap::new();
    for token in tokens {
        // Idempotent unit-weight insert: the first occurrence of any token (or any
        // token that hashes to this index) establishes weight 1.0; repeats are
        // no-ops. Do NOT accumulate — TF weighting belongs only on the document side.
        index_weight.entry(term_to_sparse_index(&token)).or_insert(1.0);
    }
    let mut indices = Vec::with_capacity(index_weight.len());
    let mut values = Vec::with_capacity(index_weight.len());
    for (idx, val) in index_weight {
        indices.push(idx);
        values.push(val);
    }
    (indices, values)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bm25::Bm25Index;

    /// The same term must always map to the same sparse index, across calls and
    /// sessions — proving that `term_to_sparse_index` is deterministic and stable.
    #[test]
    fn term_to_sparse_index_is_deterministic_for_same_term() {
        let first = term_to_sparse_index("redis");
        let second = term_to_sparse_index("redis");
        assert_eq!(
            first, second,
            "same term must produce the same index every call"
        );
    }

    /// Two different terms must (in the overwhelming majority of cases) map to
    /// different indices. This is not a strict uniqueness guarantee (FNV-1a has
    /// collisions) but is required for the pair ("redis", "tokio") because they
    /// differ in almost all bytes.
    #[test]
    fn term_to_sparse_index_produces_different_indices_for_different_terms() {
        let redis_idx = term_to_sparse_index("redis");
        let tokio_idx = term_to_sparse_index("tokio");
        assert_ne!(
            redis_idx, tokio_idx,
            "distinct terms must produce distinct indices"
        );
    }

    /// A document with higher term frequency for a term must get a higher
    /// BM25 TF-saturation weight (diminishing returns, but strictly increasing).
    #[test]
    fn higher_tf_produces_higher_sparse_weight() {
        let docs = vec![
            (0_usize, "redis redis redis".to_owned()),
            (1_usize, "redis".to_owned()),
        ];
        let index = Bm25Index::build(&docs);
        let sparse_vecs = build_skill_sparse_vectors(&docs, &index);

        let redis_idx = term_to_sparse_index("redis");

        let weight_high_tf = sparse_vecs[0]
            .0
            .iter()
            .zip(sparse_vecs[0].1.iter())
            .find(|&(i, _)| *i == redis_idx)
            .map(|(_, w)| *w)
            .expect("doc 0 must contain 'redis' index");

        let weight_low_tf = sparse_vecs[1]
            .0
            .iter()
            .zip(sparse_vecs[1].1.iter())
            .find(|&(i, _)| *i == redis_idx)
            .map(|(_, w)| *w)
            .expect("doc 1 must contain 'redis' index");

        assert!(
            weight_high_tf > weight_low_tf,
            "doc with tf=3 must outscore doc with tf=1: got {weight_high_tf} vs {weight_low_tf}"
        );
    }

    /// `query_sparse_vector` must de-duplicate repeated query terms so each
    /// unique token contributes exactly weight `1.0` at its index, regardless of how
    /// many times the term appears in the query string.
    ///
    /// This matches the Qdrant `modifier: idf` convention: the client sends unit
    /// weights; IDF scaling is applied server-side. Accumulating TF on the query side
    /// would over-weight repeated terms before the server applies IDF.
    #[test]
    fn query_sparse_vector_deduplicates_repeated_terms() {
        // "redis" appears three times, "async" once — should yield 2 distinct indices,
        // each with weight 1.0.
        let (indices, values) = query_sparse_vector("redis redis redis async");
        assert_eq!(
            indices.len(),
            values.len(),
            "indices and values must have equal length"
        );
        // Two distinct tokens: "redis" and "async".
        assert_eq!(
            indices.len(),
            2,
            "three 'redis' + one 'async' must collapse to 2 distinct index entries"
        );
        // Every weight must be exactly 1.0 — repeated occurrences must not accumulate.
        for &w in &values {
            assert_eq!(
                w, 1.0,
                "each distinct query term must contribute weight 1.0, got {w}"
            );
        }
    }

    /// An empty query must produce empty slices without panic.
    #[test]
    fn query_sparse_vector_empty_query_produces_empty_output() {
        let (indices, values) = query_sparse_vector("");
        assert!(indices.is_empty());
        assert!(values.is_empty());
    }

    /// A skill with no tokens must produce an empty sparse vector without panic.
    #[test]
    fn build_skill_sparse_vectors_empty_doc_produces_empty_vector() {
        let docs = vec![(0_usize, String::new())];
        let index = Bm25Index::build(&docs);
        let sparse_vecs = build_skill_sparse_vectors(&docs, &index);
        assert_eq!(sparse_vecs.len(), 1);
        assert!(
            sparse_vecs[0].0.is_empty(),
            "empty doc must produce empty indices"
        );
        assert!(
            sparse_vecs[0].1.is_empty(),
            "empty doc must produce empty values"
        );
    }
}
