//! Shared lexical tokenization for the retrieval read path.
//!
//! A single raw tokenizer is the source of truth for how BM25 indexing, BM25
//! query scoring, sparse-vector TF counting, and graph-search token overlap
//! split text into terms. Keeping ONE definition is a hard invariant: the
//! write side (`build_skill_sparse_vectors`, `Bm25Index::build`) and the read
//! side (`Bm25Index::score`, `query_sparse_vector`, `graph_search`) must
//! tokenize identically, or TF/IDF alignment between the in-memory BM25 index
//! and the Qdrant sparse vectors silently corrupts. Any future tokenization
//! policy change (stemming, stop-words, unicode handling) is made HERE, once,
//! and every caller inherits it (#249; same blast radius as #246's shared
//! `skill_lexical_document`).

/// Splits text into the raw, ordered token stream: split on any
/// non-alphanumeric character, trim, lowercase, and drop empty tokens.
///
/// Duplicates are preserved so callers that need term frequency (BM25 indexing,
/// sparse TF vectors, document-length counts) count repetitions correctly.
/// Callers that want a deduplicated set collect the result into a
/// `BTreeSet`/`HashSet` — they must NOT re-implement the split.
pub(crate) fn tokenize_tokens(text: &str) -> Vec<String> {
    text.split(|ch: char| !ch.is_alphanumeric())
        .map(|token| token.trim().to_lowercase())
        .filter(|token| !token.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_lowercases_and_drops_empties_keeping_duplicates() {
        let toks = tokenize_tokens("Foo-Bar  foo, BAZ!!");
        assert_eq!(toks, vec!["foo", "bar", "foo", "baz"]);
    }

    #[test]
    fn empty_and_punctuation_only_input_yields_no_tokens() {
        assert!(tokenize_tokens("").is_empty());
        assert!(tokenize_tokens("---  .,!").is_empty());
    }
}
