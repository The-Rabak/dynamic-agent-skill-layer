//! Loader for the labeled relevance corpus and the in-harness pure-lexical
//! ranking baseline.
//!
//! The baseline exists to answer the one question the product's name implies but
//! has never been measured: **does semantic subunit retrieval rank better than
//! keyword matching?** `lexical_baseline_ranking` reproduces production's
//! `token_overlap_score` (`crates/retrieval/src/graph_search.rs`) faithfully —
//! it is a measurement reference, NOT a fake of production. The live harness
//! compares it against the ranking parsed from a REAL `compile_context` call.
#![allow(dead_code)]

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::Deserialize;

/// The full labeled corpus: skills to seed, queries with ground-truth labels,
/// and the honest thresholds the harness asserts.
#[derive(Debug, Clone, Deserialize)]
pub struct LabeledCorpus {
    pub skills: Vec<LabeledSkill>,
    pub queries: Vec<LabeledQuery>,
    #[serde(rename = "_thresholds")]
    pub thresholds: Thresholds,
}

/// A skill definition seeded into the real graph via the harness sidecar.
#[derive(Debug, Clone, Deserialize)]
pub struct LabeledSkill {
    pub id: String,
    pub description: String,
    pub tags: Vec<String>,
    pub subunits: Vec<LabeledSubunit>,
}

/// One subunit (procedure / convention / asset) of a labeled skill.
#[derive(Debug, Clone, Deserialize)]
pub struct LabeledSubunit {
    pub kind: String,
    pub title: String,
    pub content: String,
}

/// A graded query: `relevant` names the skill id(s) that are correct answers.
#[derive(Debug, Clone, Deserialize)]
pub struct LabeledQuery {
    pub id: String,
    /// `"lexical"`, `"disjoint"`, or `"negative"`.
    pub kind: String,
    pub text: String,
    pub relevant: Vec<String>,
}

impl LabeledQuery {
    /// Ground-truth relevant ids as a set.
    pub fn relevant_set(&self) -> BTreeSet<String> {
        self.relevant.iter().cloned().collect()
    }
}

/// Honest assertion bars read from the fixture (not hard-coded in the test).
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Thresholds {
    pub mean_mrr_min: f64,
    pub mean_precision_at_1_min: f64,
    pub mean_ndcg_at_3_min: f64,
    pub disjoint_recall_at_3_min: f64,
    pub semantic_must_beat_lexical_map_on_disjoint: bool,
    pub negative_max_false_match_rate: f64,
    pub latency_p95_ms_budget: u64,
}

/// Loads the labeled corpus from `tests/fixtures/retrieval_quality_labeled.json`.
///
/// Resolves the path from the including crate's `CARGO_MANIFEST_DIR` (the test is
/// registered under `crates/mcp-server`, two levels below the repo root). Fails
/// loud — a missing or malformed fixture panics rather than silently yielding an
/// empty corpus that would make the quality suite vacuously pass.
pub fn load() -> LabeledCorpus {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/fixtures/retrieval_quality_labeled.json");

    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "retrieval-quality harness: could not read labeled corpus at {}: {e}",
            path.display()
        )
    });

    let corpus: LabeledCorpus = serde_json::from_str(&raw).unwrap_or_else(|e| {
        panic!("retrieval-quality harness: labeled corpus is malformed JSON: {e}")
    });

    assert!(
        !corpus.skills.is_empty() && !corpus.queries.is_empty(),
        "retrieval-quality harness: labeled corpus must define skills and queries"
    );
    corpus
}

impl LabeledSkill {
    /// Renders this skill as `SKILL.md` content for sidecar seeding.
    ///
    /// `heading` becomes the H1, which graph-builder uses as the skill name and
    /// which the compiler renders as `## Skill: <heading>` — so the harness can
    /// parse the ranked skill ids back out of `additional_context`. Procedures
    /// and conventions are emitted under the headings the extractor recognises.
    pub fn skill_md(&self, heading: &str) -> String {
        let mut md = format!(
            "# {heading}\ntags: {}\n\n{}\n",
            self.tags.join(", "),
            self.description
        );

        let procedures: Vec<&LabeledSubunit> = self
            .subunits
            .iter()
            .filter(|s| s.kind == "procedure")
            .collect();
        let conventions: Vec<&LabeledSubunit> = self
            .subunits
            .iter()
            .filter(|s| s.kind != "procedure")
            .collect();

        if !procedures.is_empty() {
            md.push_str("\n## Procedures\n");
            for s in procedures {
                md.push_str(&format!("- {}: {}\n", s.title, s.content));
            }
        }
        if !conventions.is_empty() {
            md.push_str("\n## Conventions\n");
            for s in conventions {
                md.push_str(&format!("- {}: {}\n", s.title, s.content));
            }
        }
        md
    }

    /// The full searchable text a keyword matcher would index for this skill:
    /// id words + description + tags + every subunit's title and content.
    fn searchable_text(&self) -> String {
        let mut parts = vec![
            self.id.replace('-', " "),
            self.description.clone(),
            self.tags.join(" "),
        ];
        for s in &self.subunits {
            parts.push(s.title.clone());
            parts.push(s.content.clone());
        }
        parts.join(" ")
    }
}

/// Conservative English function-word stoplist (articles, prepositions,
/// conjunctions, pronouns, auxiliaries). Removing these makes the lexical
/// baseline a STRONGER keyword competitor — it matches on content words rather
/// than being swamped by the stopwords every document shares — so "semantic
/// beats lexical" is a harder, more honest bar. Only clearly-functional words
/// are listed; information-bearing words are never stopped.
const STOPWORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "by", "can", "could", "do", "does", "for", "from",
    "has", "have", "how", "i", "in", "into", "is", "it", "its", "of", "off", "on", "or", "out",
    "over", "own", "so", "that", "the", "their", "them", "then", "there", "they", "this", "to",
    "up", "was", "were", "will", "with", "while", "when", "where", "what", "which", "but", "not",
    "no", "nor", "if", "than", "under", "my",
];

/// Tokenizes like production (`graph_search::tokenize`): split on non-alphanumeric,
/// lowercase, drop empties. Used for the disjointness diagnostics.
pub fn tokenize(input: &str) -> BTreeSet<String> {
    input
        .split(|ch: char| !ch.is_alphanumeric())
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty())
        .collect()
}

/// Content tokens: `tokenize` minus the conservative stoplist. The lexical
/// baseline ranks on these so it competes on meaning-bearing words.
fn content_tokens(input: &str) -> BTreeSet<String> {
    let stop: BTreeSet<&str> = STOPWORDS.iter().copied().collect();
    tokenize(input)
        .into_iter()
        .filter(|t| !stop.contains(t.as_str()))
        .collect()
}

/// Production-faithful token-overlap score: |query ∩ doc| / |query|.
fn token_overlap(query_tokens: &BTreeSet<String>, doc_tokens: &BTreeSet<String>) -> f64 {
    if query_tokens.is_empty() || doc_tokens.is_empty() {
        return 0.0;
    }
    query_tokens.intersection(doc_tokens).count() as f64 / query_tokens.len() as f64
}

/// Ranks every skill id by pure lexical token overlap with `query_text`,
/// descending; ties break by id for determinism.
///
/// This is the keyword-matching baseline the semantic system must beat. A
/// disjoint query (one that shares no literal tokens with its relevant skill)
/// will rank that skill near the bottom here — by construction.
pub fn lexical_baseline_ranking(query_text: &str, skills: &[LabeledSkill]) -> Vec<String> {
    let query_tokens = content_tokens(query_text);
    let mut scored: Vec<(String, f64)> = skills
        .iter()
        .map(|s| {
            (
                s.id.clone(),
                token_overlap(&query_tokens, &content_tokens(&s.searchable_text())),
            )
        })
        .collect();

    scored.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    scored.into_iter().map(|(id, _)| id).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corpus_loads_and_is_internally_consistent() {
        let corpus = load();
        let ids: BTreeSet<String> = corpus.skills.iter().map(|s| s.id.clone()).collect();

        // Every relevant id named by a query must correspond to a real skill.
        for q in &corpus.queries {
            for r in &q.relevant {
                assert!(
                    ids.contains(r),
                    "query {} references unknown skill {r}",
                    q.id
                );
            }
            assert!(
                matches!(q.kind.as_str(), "lexical" | "disjoint" | "negative"),
                "query {} has unknown kind {}",
                q.id,
                q.kind
            );
            if q.kind == "negative" {
                assert!(
                    q.relevant.is_empty(),
                    "negative query {} must have no relevant",
                    q.id
                );
            } else {
                assert!(
                    !q.relevant.is_empty(),
                    "{} query {} needs a relevant skill",
                    q.kind,
                    q.id
                );
            }
        }

        // We need both lexical and disjoint coverage for the comparison to mean anything.
        assert!(corpus.queries.iter().any(|q| q.kind == "lexical"));
        assert!(corpus.queries.iter().any(|q| q.kind == "disjoint"));
        assert!(corpus.queries.iter().any(|q| q.kind == "negative"));
    }

    #[test]
    fn skill_md_roundtrips_heading_into_skill_marker_format() {
        let corpus = load();
        let skill = &corpus.skills[0];
        let md = skill.skill_md("ns-rust-async-file-io");
        assert!(md.starts_with("# ns-rust-async-file-io\n"));
        assert!(md.contains("## Procedures"));
        assert!(md.contains("## Conventions"));
    }

    #[test]
    fn lexical_baseline_ranks_lexical_target_high() {
        let corpus = load();
        let q = corpus
            .queries
            .iter()
            .find(|q| q.id == "q-lex-file")
            .unwrap();
        let ranking = lexical_baseline_ranking(&q.text, &corpus.skills);
        // A heavily-overlapping query should put its target at rank 1.
        assert_eq!(
            ranking.first().map(String::as_str),
            Some("rust-async-file-io")
        );
    }

    /// Reciprocal rank of `target` in a ranking (0 if absent).
    fn rr(ranking: &[String], target: &str) -> f64 {
        ranking
            .iter()
            .position(|id| id == target)
            .map(|i| 1.0 / (i as f64 + 1.0))
            .unwrap_or(0.0)
    }

    #[test]
    fn disjoint_queries_genuinely_handicap_keyword_matching() {
        // PREMISE of the live semantic-vs-lexical comparison: the disjoint query set
        // must be hard for pure keyword matching (otherwise "semantic beats lexical"
        // is unwinnable/meaningless). We assert the aggregate, not a single query:
        //   • every lexical query's target is rank 1 lexically (RR = 1.0), and
        //   • the disjoint set's mean lexical RR is far lower, and
        //   • NO disjoint target is rank 1 lexically.
        // If this regresses, the fixture has drifted toward lexical overlap and the
        // live comparison would flatter the semantic pipeline — fix the fixture.
        let corpus = load();

        let mut lexical_rr = Vec::new();
        let mut disjoint_rr = Vec::new();
        let mut disjoint_at_rank_one = 0;

        for q in &corpus.queries {
            if q.relevant.is_empty() {
                continue;
            }
            let ranking = lexical_baseline_ranking(&q.text, &corpus.skills);
            let target = &q.relevant[0];
            let r = rr(&ranking, target);
            match q.kind.as_str() {
                "lexical" => lexical_rr.push(r),
                "disjoint" => {
                    disjoint_rr.push(r);
                    if ranking.first() == Some(target) {
                        disjoint_at_rank_one += 1;
                    }
                }
                _ => {}
            }
        }

        let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len().max(1) as f64;
        let lex_mean = mean(&lexical_rr);
        let dis_mean = mean(&disjoint_rr);

        assert!(
            (lex_mean - 1.0).abs() < 1e-9,
            "lexical queries should each rank their target #1 (mean RR {lex_mean:.3})"
        );
        assert_eq!(
            disjoint_at_rank_one, 0,
            "no disjoint target may rank #1 under keyword matching — fixture not disjoint enough"
        );
        assert!(
            lex_mean - dis_mean > 0.4,
            "disjoint set must be much harder for keyword matching: lexical RR {lex_mean:.3} vs disjoint RR {dis_mean:.3}"
        );
    }
}
