/// Real Okapi BM25 index for in-memory lexical skill retrieval.
///
/// BM25 parameters:
/// - `k1 = 1.2`: term-frequency saturation constant. Controls how much repeated
///   occurrences of a query term in a document boost its score (diminishing returns).
/// - `b = 0.75`: document-length normalization constant. `b=1` fully penalizes long
///   documents; `b=0` disables length normalization.
///
/// IDF formula (Robertson-Sparck Jones, smooth variant that avoids negatives):
///   `IDF(t) = ln((N - df(t) + 0.5) / (df(t) + 0.5) + 1)`
///
/// where `N` is the corpus size and `df(t)` is the number of documents containing
/// term `t`. The `+ 1` inside the `ln` guarantees non-negative IDF even when
/// `df(t) == N`.
///
/// BM25 score for document `d` against query `Q`:
///   `score(d, Q) = Σ_{t ∈ Q} IDF(t) · (tf(t,d) · (k1+1)) / (tf(t,d) + k1·(1 - b + b·|d|/avgdl))`
///
/// where `tf(t,d)` is the term frequency in document `d` and `|d|/avgdl` is the
/// document-length ratio. This is a standard dependency-free, hand-rolled Okapi BM25.
use std::collections::HashMap;

/// Default BM25 term-frequency saturation constant.
const BM25_K1: f32 = 1.2;
/// Default BM25 document-length normalization constant.
const BM25_B: f32 = 0.75;

/// In-memory Okapi BM25 index over a skill corpus.
///
/// Built once at snapshot construction time and stored on the `RetrievalSnapshot`
/// under an `Arc` so it swaps atomically alongside the graph. Cheap to build
/// (~microseconds for a 5000-skill corpus) so it is constructed unconditionally,
/// allowing `RETRIEVAL_BACKEND` to switch between dense-only and hybrid at
/// request time without a graph rebuild.
///
/// The index is read-only after construction; no mutation methods are exposed.
#[derive(Debug, Clone)]
pub struct Bm25Index {
    /// For each term: the number of documents (skills) containing it.
    doc_frequency: HashMap<String, usize>,
    /// For each document (by the `skill_index` passed to `build`): term frequencies.
    term_frequencies: HashMap<usize, HashMap<String, usize>>,
    /// Length of each document in tokens.
    doc_lengths: HashMap<usize, usize>,
    /// Average document length across the corpus (in tokens).
    avg_doc_length: f32,
    /// Total number of documents in the corpus.
    corpus_size: usize,
    /// BM25 k1 parameter.
    k1: f32,
    /// BM25 b parameter.
    b: f32,
}

impl Bm25Index {
    /// Builds a BM25 index from a slice of `(skill_index, document_text)` pairs.
    ///
    /// `skill_index` is the index into `RetrievalSnapshot.skills` — the same
    /// integer used throughout the retrieval read path. `document_text` is the
    /// pre-built lexical document for that skill (name + tags + description +
    /// multi-view fields + subunit titles/content).
    ///
    /// Returns an empty-but-valid index when `docs` is empty (no skills loaded);
    /// `score()` will return an empty result vector in that case.
    pub fn build(docs: &[(usize, String)]) -> Self {
        let mut doc_frequency: HashMap<String, usize> = HashMap::new();
        let mut term_frequencies: HashMap<usize, HashMap<String, usize>> = HashMap::new();
        let mut doc_lengths: HashMap<usize, usize> = HashMap::new();

        for (skill_index, text) in docs {
            // Split into a raw token stream (with repetitions) for accurate TF and
            // document-length counts. `tokenize()` returns a BTreeSet which deduplicates
            // — we cannot use it directly for TF or doc-length here.
            let tokens: Vec<String> = text
                .split(|ch: char| !ch.is_alphanumeric())
                .map(|t| t.trim().to_lowercase())
                .filter(|t| !t.is_empty())
                .collect();
            let token_count = tokens.len();
            doc_lengths.insert(*skill_index, token_count);

            let mut local_freq: HashMap<String, usize> = HashMap::new();
            for token in &tokens {
                *local_freq.entry(token.clone()).or_insert(0) += 1;
            }
            // Count this document for each distinct term it contains.
            for term in local_freq.keys() {
                *doc_frequency.entry(term.clone()).or_insert(0) += 1;
            }
            term_frequencies.insert(*skill_index, local_freq);
        }

        let corpus_size = docs.len();
        let avg_doc_length = if corpus_size == 0 {
            0.0
        } else {
            doc_lengths.values().sum::<usize>() as f32 / corpus_size as f32
        };

        Self {
            doc_frequency,
            term_frequencies,
            doc_lengths,
            avg_doc_length,
            corpus_size,
            k1: BM25_K1,
            b: BM25_B,
        }
    }

    /// Returns the average document length across the corpus (in tokens).
    ///
    /// Used by the sparse write path (`build_skill_sparse_vectors`) to compute
    /// the BM25 length-normalization factor without duplicating the corpus build.
    /// Returns `0.0` for an empty corpus.
    pub fn avg_doc_length(&self) -> f32 {
        self.avg_doc_length
    }

    /// Returns a reference to the per-document token-count map.
    ///
    /// Keys are the same `skill_index` values passed to `build`. Used by the sparse
    /// write path so it can look up each skill's document length without recomputing
    /// the tokenization. Read-only; the index is immutable after construction.
    pub fn doc_lengths_map(&self) -> &HashMap<usize, usize> {
        &self.doc_lengths
    }

    /// Computes BM25 scores for all indexed documents against `query_terms`.
    ///
    /// Only documents in `candidate_indices` are scored; pass all document
    /// indices to score the full corpus. Documents with a zero total BM25 score
    /// (no matching terms) are omitted from the result.
    ///
    /// Returns `Vec<(skill_index, bm25_score)>` sorted descending by score.
    pub fn score(&self, query_terms: &[String], candidate_indices: &[usize]) -> Vec<(usize, f32)> {
        if self.corpus_size == 0 || query_terms.is_empty() {
            return Vec::new();
        }

        let mut scores: Vec<(usize, f32)> = candidate_indices
            .iter()
            .filter_map(|&skill_index| {
                let tf_map = self.term_frequencies.get(&skill_index)?;
                let doc_len = *self.doc_lengths.get(&skill_index)? as f32;
                let length_norm = 1.0 - self.b + self.b * (doc_len / self.avg_doc_length.max(1.0));

                let bm25_score: f32 = query_terms
                    .iter()
                    .map(|term| {
                        let df = self.doc_frequency.get(term).copied().unwrap_or(0);
                        if df == 0 {
                            return 0.0;
                        }
                        let n = self.corpus_size as f32;
                        // Smooth Robertson-Sparck Jones IDF — always non-negative.
                        let idf = ((n - df as f32 + 0.5) / (df as f32 + 0.5) + 1.0).ln();
                        let tf = tf_map.get(term).copied().unwrap_or(0) as f32;
                        let tf_norm = tf * (self.k1 + 1.0) / (tf + self.k1 * length_norm);
                        idf * tf_norm
                    })
                    .sum();

                if bm25_score > 0.0 {
                    Some((skill_index, bm25_score))
                } else {
                    None
                }
            })
            .collect();

        scores.sort_by(|left, right| right.1.total_cmp(&left.1));
        scores
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A doc containing a rare query term must outscore one without it.
    /// This is the fundamental BM25 contract: rare terms (high IDF) drive
    /// the ranking more than common terms.
    #[test]
    fn bm25_rare_term_doc_outranks_doc_without_term() {
        let docs = vec![
            // Doc 0: contains the rare term "tokio" once (+ common terms)
            (0_usize, "tokio async runtime".to_owned()),
            // Doc 1: no "tokio" — only common terms
            (1_usize, "async runtime".to_owned()),
            // Doc 2: also no "tokio"
            (2_usize, "async networking".to_owned()),
        ];
        let index = Bm25Index::build(&docs);
        let query_terms: Vec<String> = vec!["tokio".to_owned()];
        let results = index.score(&query_terms, &[0, 1, 2]);

        // Only doc 0 matches "tokio"; docs 1 and 2 score 0 and are omitted.
        assert_eq!(
            results.len(),
            1,
            "only the doc containing 'tokio' should have a non-zero score"
        );
        assert_eq!(
            results[0].0, 0,
            "the doc with 'tokio' (index=0) must rank first"
        );
        assert!(
            results[0].1 > 0.0,
            "BM25 score for matching doc must be > 0"
        );
    }

    /// A doc with higher term frequency of a query term must outscore one with lower
    /// frequency (all else equal), demonstrating TF saturation.
    #[test]
    fn bm25_higher_tf_outscores_lower_tf_for_same_term() {
        let docs = vec![
            // Doc 0: "redis" appears twice
            (0_usize, "redis redis client".to_owned()),
            // Doc 1: "redis" appears once
            (1_usize, "redis client".to_owned()),
        ];
        let index = Bm25Index::build(&docs);
        let query_terms: Vec<String> = vec!["redis".to_owned()];
        let results = index.score(&query_terms, &[0, 1]);

        assert_eq!(results.len(), 2, "both docs contain 'redis'");
        assert_eq!(
            results[0].0, 0,
            "doc with higher TF (index=0) must rank first"
        );
        assert!(
            results[0].1 > results[1].1,
            "higher TF must produce higher BM25 score (got {} vs {})",
            results[0].1,
            results[1].1
        );
    }

    /// IDF behavior: a term appearing in every doc has IDF=ln(1+0.5/N+0.5) ≈ small
    /// positive; a term appearing in only one doc has high IDF.
    #[test]
    fn bm25_idf_penalizes_common_terms_relative_to_rare_terms() {
        // Four docs: "rust" appears in all 4; "serde" appears in only 1.
        let docs = vec![
            (0_usize, "rust serde json".to_owned()),
            (1_usize, "rust async io".to_owned()),
            (2_usize, "rust tokio net".to_owned()),
            (3_usize, "rust clippy lint".to_owned()),
        ];
        let index = Bm25Index::build(&docs);

        // Score with the rare term "serde": only doc 0 matches.
        let rare_query = vec!["serde".to_owned()];
        let rare_results = index.score(&rare_query, &[0, 1, 2, 3]);

        // Score with the common term "rust": all docs match but with lower IDF.
        let common_query = vec!["rust".to_owned()];
        let common_results = index.score(&common_query, &[0, 1, 2, 3]);

        // "serde" appears in only 1 of 4 docs → high IDF → high score for that doc.
        // "rust" appears in all 4 docs → low IDF → each doc gets a modest score.
        // The top "serde" hit should outscore the top "rust" hit.
        assert!(
            !rare_results.is_empty(),
            "rare term 'serde' must match doc 0"
        );
        assert!(!common_results.is_empty(), "common term 'rust' must match");
        assert!(
            rare_results[0].1 > common_results[0].1,
            "rare term must produce higher BM25 score than universal term: serde={} vs rust={}",
            rare_results[0].1,
            common_results[0].1
        );
    }

    /// A candidate_indices filter restricts scoring to the specified subset.
    #[test]
    fn bm25_candidate_indices_filter_restricts_scored_docs() {
        let docs = vec![
            (0_usize, "tokio async runtime".to_owned()),
            (1_usize, "tokio client".to_owned()),
            (2_usize, "sqlx postgres".to_owned()),
        ];
        let index = Bm25Index::build(&docs);
        let query = vec!["tokio".to_owned()];

        // Score only docs 1 and 2 — doc 0 must not appear even though it matches.
        let filtered = index.score(&query, &[1, 2]);
        assert_eq!(filtered.len(), 1, "only doc 1 matches 'tokio' among [1, 2]");
        assert_eq!(filtered[0].0, 1, "doc 1 must be the result");
    }

    /// Empty corpus returns an empty result without panic.
    #[test]
    fn bm25_empty_corpus_returns_empty_scores() {
        let index = Bm25Index::build(&[]);
        let results = index.score(&["tokio".to_owned()], &[]);
        assert!(results.is_empty());
    }

    /// Empty query terms return an empty result without panic.
    #[test]
    fn bm25_empty_query_returns_empty_scores() {
        let docs = vec![(0_usize, "tokio async".to_owned())];
        let index = Bm25Index::build(&docs);
        let results = index.score(&[], &[0]);
        assert!(results.is_empty());
    }
}
