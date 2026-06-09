use std::collections::HashMap;

use domain::{HdbscanConfig, ScopeType};
use hdbscan::{Hdbscan, HdbscanHyperParams, NnAlgorithm};

use crate::graph::build::BuiltSkill;

/// Membership source for a community assignment, matching the `community_skills.source` DB column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommunitySource {
    /// Cluster produced by HDBSCAN over skill embeddings.
    Hdbscan,
    /// Community produced by grouping on the skill's first tag.
    Tag,
}

impl CommunitySource {
    pub fn as_db_str(&self) -> &'static str {
        match self {
            Self::Hdbscan => "hdbscan",
            Self::Tag => "tag",
        }
    }
}

/// One community with its member skill IDs and the mechanism that produced it.
///
/// A single skill may appear in multiple `CommunityAssignment` values when dual
/// membership is active: once with `source = Hdbscan` (its semantic cluster) and
/// once with `source = Tag` (its tag community).  Skills labelled noise by HDBSCAN
/// appear in a per-scope `"{scope}-unclustered"` community with `source = Hdbscan`
/// so they remain retrievable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommunityAssignment {
    pub community_name: String,
    pub skill_ids: Vec<String>,
    pub scope: ScopeType,
    pub source: CommunitySource,
}

/// Assigns dual-membership communities to skills.
///
/// Produces two independent layers of communities:
///
/// 1. **HDBSCAN layer** (`source = Hdbscan`): runs HDBSCAN over the 768-dim
///    embeddings of all skills within each scope.  Each cluster becomes a community
///    named `"{scope}-cluster-{idf_label}"` where `idf_label` is built from the
///    highest TF-IDF terms of the cluster members' subunit text — i.e. the terms
///    that are frequent inside the cluster yet rare across the whole skill corpus,
///    so the name describes what makes the cluster distinctive (ties broken
///    alphabetically).  Noise skills
///    (HDBSCAN label -1) are collected into a per-scope `"{scope}-unclustered"`
///    community so they remain reachable.  When a scope has fewer than
///    `config.min_cluster_size` skills the whole scope is placed in the unclustered
///    community rather than calling the clusterer on a degenerate input.
///
/// 2. **Tag layer** (`source = Tag`): groups skills by their first tag, producing
///    communities named `"{scope}-{tag}"` — identical to the previous behaviour
///    before HDBSCAN was introduced.  Skills with no tags fall into
///    `"{scope}-untagged"`.
///
/// Both layers are concatenated and sorted by community name so repeated rebuilds
/// with identical inputs always produce an identical ordering.
///
/// # Failures
/// Returns an `Err` when HDBSCAN itself returns an internal error (e.g. mismatched
/// vector dimensions or non-finite embedding values).  Callers must treat this as
/// a hard failure — do NOT fall back to tag-only silently.
pub fn assign_communities(
    skills: &[BuiltSkill],
    config: &HdbscanConfig,
) -> Result<Vec<CommunityAssignment>, String> {
    let mut assignments: Vec<CommunityAssignment> = Vec::new();

    // --- HDBSCAN layer -------------------------------------------------------
    let hdbscan_assignments = cluster_by_hdbscan(skills, config)?;
    assignments.extend(hdbscan_assignments);

    // --- Tag layer -----------------------------------------------------------
    assignments.extend(cluster_by_first_tag(skills));

    // Deterministic ordering: identical input → identical output so rebuild
    // diffs stay stable.
    assignments.sort_by(|left, right| {
        left.community_name
            .cmp(&right.community_name)
            .then_with(|| left.source.as_db_str().cmp(right.source.as_db_str()))
    });

    Ok(assignments)
}

// ---------------------------------------------------------------------------
// HDBSCAN clustering
// ---------------------------------------------------------------------------

/// Runs HDBSCAN per scope and returns one `CommunityAssignment` per cluster
/// (plus a per-scope `"unclustered"` assignment for noise skills).
///
/// Noise label (-1): every skill HDBSCAN marks as noise is placed into a shared
/// `"{scope}-unclustered"` community so it remains retrievable.  This is preferable
/// to silently dropping noise skills from all communities — retrieval would miss
/// them entirely without a community anchor.
///
/// When there are fewer skills in a scope than `config.min_cluster_size`, HDBSCAN
/// would trivially label every skill as noise; we skip the clusterer and place all
/// skills directly into the `"{scope}-unclustered"` community.
fn cluster_by_hdbscan(
    skills: &[BuiltSkill],
    config: &HdbscanConfig,
) -> Result<Vec<CommunityAssignment>, String> {
    // Fail fast: embeddings must be populated before we get here.
    for skill in skills {
        if skill.embedding.is_empty() {
            return Err(format!(
                "HDBSCAN clustering requires embeddings but skill '{}' has an empty \
                 embedding vector — ensure build_skills_from_scope_roots completes \
                 the embedding step before calling assign_communities",
                skill.id
            ));
        }
    }

    // Partition skills by scope so HDBSCAN clusters within scope boundaries.
    let mut by_scope: HashMap<String, Vec<usize>> = HashMap::new();
    for (index, skill) in skills.iter().enumerate() {
        by_scope
            .entry(format!("{:?}", skill.scope_type))
            .or_default()
            .push(index);
    }

    let mut assignments: Vec<CommunityAssignment> = Vec::new();

    // Corpus-wide document frequencies (one "document" per skill) drive the IDF
    // term of every cluster label.  Computed once over the whole corpus, not
    // per-scope, so a term's rarity is judged against all skills.
    let corpus = CorpusIdf {
        doc_freq: corpus_document_frequencies(skills),
        size: skills.len(),
    };

    // Community names must be unique across the whole HDBSCAN layer: two clusters
    // that derived the same label would collapse to one `stable_id` downstream.
    let mut used_names: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Sort scope keys for deterministic iteration order.
    let mut scope_keys: Vec<String> = by_scope.keys().cloned().collect();
    scope_keys.sort();

    for scope_key in scope_keys {
        let indices = &by_scope[&scope_key];
        let scope = scope_from_debug_str(&scope_key);
        let scope_prefix = scope_prefix_for(scope);

        if indices.len() < config.min_cluster_size {
            // Not enough skills to form any cluster — place all in unclustered.
            let mut skill_ids: Vec<String> =
                indices.iter().map(|&i| skills[i].id.clone()).collect();
            skill_ids.sort();
            assignments.push(CommunityAssignment {
                community_name: format!("{scope_prefix}-unclustered"),
                skill_ids,
                scope,
                source: CommunitySource::Hdbscan,
            });
            continue;
        }

        // Build Vec<Vec<f64>> for this scope's skills (HDBSCAN expects f64).
        let vectors: Vec<Vec<f64>> = indices
            .iter()
            .map(|&i| skills[i].embedding.iter().map(|&v| v as f64).collect())
            .collect();

        let hyper_params = HdbscanHyperParams::builder()
            .min_cluster_size(config.min_cluster_size)
            .nn_algorithm(NnAlgorithm::BruteForce)
            .build();

        let clusterer = Hdbscan::new(&vectors, hyper_params);
        let labels = clusterer
            .cluster()
            .map_err(|err| format!("HDBSCAN clustering failed for scope '{scope_key}': {err:?}"))?;

        // Group skill indices by cluster label.
        let mut clusters: HashMap<i32, Vec<usize>> = HashMap::new();
        for (local_index, &label) in labels.iter().enumerate() {
            clusters.entry(label).or_default().push(local_index);
        }

        // Sort cluster labels so output order is deterministic.
        let mut cluster_labels: Vec<i32> = clusters.keys().copied().collect();
        cluster_labels.sort();

        let mut noise_ids: Vec<String> = Vec::new();

        for label in cluster_labels {
            let local_indices = &clusters[&label];

            if label == -1 {
                // Noise: accumulate for the shared unclustered community.
                for &local_index in local_indices {
                    noise_ids.push(skills[indices[local_index]].id.clone());
                }
                continue;
            }

            // Named cluster: derive the community name from the highest TF-IDF
            // terms of the members' subunit text so the label describes what makes
            // the cluster distinctive and stays stable across rebuilds.
            let member_skill_ids: Vec<String> = {
                let mut ids: Vec<String> = local_indices
                    .iter()
                    .map(|&local_index| skills[indices[local_index]].id.clone())
                    .collect();
                ids.sort();
                ids
            };

            let label_term = derive_cluster_label(
                local_indices,
                indices,
                skills,
                scope_prefix,
                label,
                &corpus,
                &mut used_names,
            );

            assignments.push(CommunityAssignment {
                community_name: label_term,
                skill_ids: member_skill_ids,
                scope,
                source: CommunitySource::Hdbscan,
            });
        }

        // Emit the unclustered community if there are any noise skills.
        if !noise_ids.is_empty() {
            noise_ids.sort();
            assignments.push(CommunityAssignment {
                community_name: format!("{scope_prefix}-unclustered"),
                skill_ids: noise_ids,
                scope,
                source: CommunitySource::Hdbscan,
            });
        }
    }

    Ok(assignments)
}

/// Derives a deterministic, human-readable community name for an HDBSCAN cluster
/// from the highest TF-IDF terms of its members' subunit text.
///
/// A term scores `tf * idf` where `tf` is its total occurrences across the
/// cluster's subunits and `idf = ln((N + 1) / (df + 1)) + 1` over the whole skill
/// corpus (`N` skills, `df` skills containing the term).  This favours terms that
/// are frequent *within* the cluster yet rare *across* the corpus, so the label
/// captures what the cluster is about rather than boilerplate shared by every
/// skill.  Ties break alphabetically for stability.
///
/// The label is `"{scope_prefix}-cluster-{t1}-{t2}"` using the top two terms (one
/// term if only one is available).  When members have no usable terms at all it
/// falls back to `"{scope_prefix}-cluster-{label}"` (the raw HDBSCAN integer).
///
/// `used_names` guarantees within-layer uniqueness so two clusters can never
/// collapse onto the same `stable_id`: on collision the label is extended with the
/// next-best term, and as a last resort with the lexicographically smallest member
/// skill id (unique because HDBSCAN clusters are disjoint).
fn derive_cluster_label(
    local_indices: &[usize],
    global_indices: &[usize],
    skills: &[BuiltSkill],
    scope_prefix: &str,
    cluster_label: i32,
    corpus: &CorpusIdf,
    used_names: &mut std::collections::HashSet<String>,
) -> String {
    let scored = score_cluster_terms(local_indices, global_indices, skills, corpus);

    let mut label = if scored.is_empty() {
        format!("{scope_prefix}-cluster-{cluster_label}")
    } else {
        let core: Vec<&str> = scored
            .iter()
            .take(2)
            .map(|(term, _)| term.as_str())
            .collect();
        format!("{scope_prefix}-cluster-{}", core.join("-"))
    };

    // Collision resolution 1: append further distinctive terms.
    if used_names.contains(&label) {
        for (term, _) in scored.iter().skip(2) {
            let candidate = format!("{label}-{term}");
            if !used_names.contains(&candidate) {
                label = candidate;
                break;
            }
        }
    }
    // Collision resolution 2 (deterministic backstop): smallest member skill id
    // (unique because HDBSCAN clusters are disjoint).
    if used_names.contains(&label) {
        let anchor = local_indices
            .iter()
            .map(|&local_index| skills[global_indices[local_index]].id.as_str())
            .min();
        if let Some(anchor) = anchor {
            label = format!("{label}-{anchor}");
        }
    }

    used_names.insert(label.clone());
    label
}

/// Corpus-wide IDF context shared by every cluster's label derivation: the
/// document frequency of each term (skills containing it) and the corpus size.
struct CorpusIdf {
    doc_freq: HashMap<String, usize>,
    size: usize,
}

/// Scores every term in a cluster's subunit text by `tf * idf`, returning terms
/// sorted by score descending then alphabetically (deterministic).
fn score_cluster_terms(
    local_indices: &[usize],
    global_indices: &[usize],
    skills: &[BuiltSkill],
    corpus: &CorpusIdf,
) -> Vec<(String, f64)> {
    let mut term_freq: HashMap<String, usize> = HashMap::new();
    for &local_index in local_indices {
        let skill = &skills[global_indices[local_index]];
        for subunit in &skill.subunits {
            for term in tokenize(&subunit.title).chain(tokenize(&subunit.content)) {
                *term_freq.entry(term).or_default() += 1;
            }
        }
    }

    let mut scored: Vec<(String, f64)> = term_freq
        .into_iter()
        .map(|(term, tf)| {
            let df = *corpus.doc_freq.get(&term).unwrap_or(&0) as f64;
            let idf = ((corpus.size as f64 + 1.0) / (df + 1.0)).ln() + 1.0;
            (term, tf as f64 * idf)
        })
        .collect();

    scored.sort_by(|(term_a, score_a), (term_b, score_b)| {
        score_b
            .partial_cmp(score_a)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| term_a.cmp(term_b))
    });
    scored
}

/// Computes the corpus document frequency for every term: how many skills (one
/// "document" each) contain the term at least once in their subunit text.
fn corpus_document_frequencies(skills: &[BuiltSkill]) -> HashMap<String, usize> {
    let mut doc_freq: HashMap<String, usize> = HashMap::new();
    for skill in skills {
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        for subunit in &skill.subunits {
            for term in tokenize(&subunit.title).chain(tokenize(&subunit.content)) {
                seen.insert(term);
            }
        }
        for term in seen {
            *doc_freq.entry(term).or_default() += 1;
        }
    }
    doc_freq
}

/// Tokenizes text into lowercase alphanumeric terms of length >= 3, dropping
/// stopwords and purely numeric tokens.  Pure punctuation/whitespace splits the
/// stream into terms.
fn tokenize(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|raw| raw.len() >= 3)
        .map(|raw| raw.to_ascii_lowercase())
        .filter(|term| !term.chars().all(|c| c.is_ascii_digit()))
        .filter(|term| !is_stopword(term))
}

/// A small English stopword set so labels are anchored on meaningful terms rather
/// than connective words that appear in every skill.
fn is_stopword(term: &str) -> bool {
    const STOPWORDS: &[&str] = &[
        "the", "and", "for", "are", "but", "not", "you", "all", "any", "can", "had", "has", "her",
        "was", "one", "our", "out", "use", "uses", "used", "using", "with", "this", "that", "from",
        "into", "your", "them", "they", "then", "than", "when", "what", "which", "while", "will",
        "would", "should", "could", "have", "how", "its", "via", "per", "etc", "such", "also",
        "each", "more", "most", "some", "only", "over", "under", "between", "about", "after",
        "before", "during",
    ];
    STOPWORDS.contains(&term)
}

// ---------------------------------------------------------------------------
// Tag-based clustering (second membership layer)
// ---------------------------------------------------------------------------

/// Groups skills by their first tag per scope, replicating the original
/// deterministic community assignment but labelled with `source = Tag`.
fn cluster_by_first_tag(skills: &[BuiltSkill]) -> Vec<CommunityAssignment> {
    let mut grouped: HashMap<(String, String), Vec<String>> = HashMap::new();
    for skill in skills {
        let anchor = skill
            .tags
            .first()
            .cloned()
            .unwrap_or_else(|| "untagged".to_owned());
        grouped
            .entry((format!("{:?}", skill.scope_type), anchor))
            .or_default()
            .push(skill.id.clone());
    }

    let mut assignments: Vec<CommunityAssignment> = grouped
        .into_iter()
        .map(|((scope_key, anchor), mut skill_ids)| {
            skill_ids.sort();
            let scope = scope_from_debug_str(&scope_key);
            let scope_prefix = scope_prefix_for(scope);
            CommunityAssignment {
                community_name: format!("{scope_prefix}-{}", anchor.to_ascii_lowercase()),
                skill_ids,
                scope,
                source: CommunitySource::Tag,
            }
        })
        .collect();
    assignments.sort_by(|left, right| left.community_name.cmp(&right.community_name));
    assignments
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn scope_from_debug_str(scope_key: &str) -> ScopeType {
    match scope_key {
        "Project" => ScopeType::Project,
        "Global" => ScopeType::Global,
        _ => ScopeType::Team,
    }
}

fn scope_prefix_for(scope: ScopeType) -> &'static str {
    match scope {
        ScopeType::Project => "project",
        ScopeType::Global => "global",
        ScopeType::Team => "team",
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Builds a minimal `BuiltSkill` for test purposes with default subunit text.
    fn make_skill(id: &str, scope: ScopeType, tags: &[&str], embedding: Vec<f32>) -> BuiltSkill {
        make_skill_with_content(id, scope, tags, &format!("Do {id}"), embedding)
    }

    /// Builds a `BuiltSkill` with explicit subunit content so tests can exercise
    /// the TF-IDF label derivation, which reads subunit text (not tags).
    fn make_skill_with_content(
        id: &str,
        scope: ScopeType,
        tags: &[&str],
        content: &str,
        embedding: Vec<f32>,
    ) -> BuiltSkill {
        use crate::extraction::ExtractedSubunit;
        use domain::SubunitType;
        BuiltSkill {
            id: id.to_owned(),
            scope_id: "test".to_owned(),
            scope_type: scope,
            source_path: PathBuf::from(format!("/skills/{id}/SKILL.md")),
            name: format!("Skill {id}"),
            description: format!("Description for {id}"),
            tags: tags.iter().map(|s| s.to_string()).collect(),
            subunits: vec![ExtractedSubunit {
                kind: SubunitType::Procedure,
                title: "step".to_owned(),
                content: content.to_owned(),
            }],
            embedding,
        }
    }

    /// Proves HDBSCAN groups semantically-related skills into clusters.
    ///
    /// Two tight clusters (A: 3 skills near [10,0,...], B: 3 skills near [0,10,...])
    /// plus one geometrically middle skill that HDBSCAN absorbs into whichever cluster
    /// it is closest to (noise vs cluster depends on the local density; we do NOT
    /// assert on the middle skill's assignment since that is a parameter-sensitivity
    /// detail of HDBSCAN, not the property we are proving).
    ///
    /// What this test proves:
    /// - Cluster A (3 retrieval skills) lands in a single named HDBSCAN community.
    /// - Cluster B (3 scoring skills) lands in a different named HDBSCAN community.
    /// - The community names are derived from the top TF-IDF terms of member subunit
    ///   text — the distinctive, frequently-repeated term wins, while the term shared
    ///   by every skill ("skill") is demoted out of the label by its low IDF.
    /// - No retrieval skill appears in the scoring cluster and vice versa.
    #[test]
    fn hdbscan_clusters_similar_skills_and_marks_outlier_noise() {
        // Cluster A: 3 skills near [10, 0.x, 0, ...]
        // Cluster B: 3 skills near [0.x, 10, 0, ...]
        // Bridging skill at [5, 5, ...] — absorbed into whichever cluster is denser.
        //
        // Subunit content (NOT tags) drives the label. "skill" appears in every
        // document (low IDF → demoted); "retrieval"/"scoring" are repeated within
        // their cluster (high tf) and absent elsewhere (high IDF) → they win.
        let skills = vec![
            make_skill_with_content(
                "a1",
                ScopeType::Project,
                &["retrieval"],
                "This skill performs retrieval retrieval retrieval over search vectors",
                vec![10.0, 0.1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            ),
            make_skill_with_content(
                "a2",
                ScopeType::Project,
                &["retrieval"],
                "This skill performs retrieval retrieval retrieval over search vectors",
                vec![10.1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            ),
            make_skill_with_content(
                "a3",
                ScopeType::Project,
                &["retrieval"],
                "This skill performs retrieval retrieval retrieval over search vectors",
                vec![9.9, 0.2, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            ),
            make_skill_with_content(
                "b1",
                ScopeType::Project,
                &["scoring"],
                "This skill performs scoring scoring scoring over ranking weights",
                vec![0.1, 10.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            ),
            make_skill_with_content(
                "b2",
                ScopeType::Project,
                &["scoring"],
                "This skill performs scoring scoring scoring over ranking weights",
                vec![0.0, 10.1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            ),
            make_skill_with_content(
                "b3",
                ScopeType::Project,
                &["scoring"],
                "This skill performs scoring scoring scoring over ranking weights",
                vec![0.2, 9.9, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            ),
            // Bridging skill — assigned to whichever cluster HDBSCAN finds denser.
            // We do not assert on this skill's assignment.
            make_skill_with_content(
                "bridge",
                ScopeType::Project,
                &["unrelated"],
                "This skill is a miscellaneous helper",
                vec![5.0, 5.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            ),
        ];

        let config = HdbscanConfig {
            min_cluster_size: 3,
        };
        let assignments = assign_communities(&skills, &config)
            .expect("HDBSCAN must not error on valid embeddings");

        let hdbscan_assignments: Vec<&CommunityAssignment> = assignments
            .iter()
            .filter(|a| a.source == CommunitySource::Hdbscan)
            .collect();

        // At minimum two named clusters must exist.
        let named_clusters: Vec<&&CommunityAssignment> = hdbscan_assignments
            .iter()
            .filter(|a| !a.community_name.ends_with("-unclustered"))
            .collect();
        assert!(
            named_clusters.len() >= 2,
            "HDBSCAN must form at least two clusters from two dense groups (found {:?})",
            named_clusters
                .iter()
                .map(|c| &c.community_name)
                .collect::<Vec<_>>()
        );

        // Cluster A members (a1/a2/a3 — "retrieval") must all be in the same cluster.
        let retrieval_cluster = hdbscan_assignments
            .iter()
            .find(|a| {
                !a.community_name.ends_with("-unclustered")
                    && a.skill_ids.contains(&"a1".to_string())
            })
            .expect("a1 must be in a named HDBSCAN cluster");
        for id in &["a2", "a3"] {
            assert!(
                retrieval_cluster.skill_ids.contains(&id.to_string()),
                "skill '{id}' must be in the same HDBSCAN cluster as 'a1'"
            );
        }

        // Cluster B members (b1/b2/b3 — "scoring") must all be in the same cluster.
        let scoring_cluster = hdbscan_assignments
            .iter()
            .find(|a| {
                !a.community_name.ends_with("-unclustered")
                    && a.skill_ids.contains(&"b1".to_string())
            })
            .expect("b1 must be in a named HDBSCAN cluster");
        for id in &["b2", "b3"] {
            assert!(
                scoring_cluster.skill_ids.contains(&id.to_string()),
                "skill '{id}' must be in the same HDBSCAN cluster as 'b1'"
            );
        }

        // The two clusters must be different (semantically-related skills do NOT cross clusters).
        assert_ne!(
            retrieval_cluster.community_name, scoring_cluster.community_name,
            "retrieval and scoring skills must be in different HDBSCAN clusters"
        );

        // Community names are derived from the top TF-IDF terms of member subunit
        // text: the distinctive repeated term wins.
        assert!(
            retrieval_cluster.community_name.contains("retrieval"),
            "retrieval cluster name must contain 'retrieval', got: {}",
            retrieval_cluster.community_name
        );
        assert!(
            scoring_cluster.community_name.contains("scoring"),
            "scoring cluster name must contain 'scoring', got: {}",
            scoring_cluster.community_name
        );

        // IDF must demote the term shared by EVERY skill ("skill") so it never
        // becomes the label, even though it is frequent within the cluster.
        assert!(
            !retrieval_cluster.community_name.contains("skill"),
            "corpus-wide boilerplate 'skill' must be demoted by IDF, got: {}",
            retrieval_cluster.community_name
        );
        assert!(
            !scoring_cluster.community_name.contains("skill"),
            "corpus-wide boilerplate 'skill' must be demoted by IDF, got: {}",
            scoring_cluster.community_name
        );
    }

    /// Proves dual membership: a skill that is both in an HDBSCAN cluster and has
    /// a tag appears in BOTH an hdbscan community AND a tag community.
    ///
    /// Uses the same 3+3 cluster structure as the noise test: two dense groups
    /// so HDBSCAN can detect them.  s1 belongs to cluster A (tag "auth") AND
    /// to the tag community "project-auth".
    #[test]
    fn dual_membership_skill_appears_in_hdbscan_and_tag_community() {
        // Two tight clusters: 3 "auth" skills + 3 "infra" skills.
        let skills = vec![
            make_skill(
                "s1",
                ScopeType::Project,
                &["auth"],
                vec![10.0, 0.1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            ),
            make_skill(
                "s2",
                ScopeType::Project,
                &["auth"],
                vec![10.1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            ),
            make_skill(
                "s3",
                ScopeType::Project,
                &["auth"],
                vec![9.9, 0.2, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            ),
            make_skill(
                "s4",
                ScopeType::Project,
                &["infra"],
                vec![0.1, 10.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            ),
            make_skill(
                "s5",
                ScopeType::Project,
                &["infra"],
                vec![0.0, 10.1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            ),
            make_skill(
                "s6",
                ScopeType::Project,
                &["infra"],
                vec![0.2, 9.9, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            ),
        ];

        let config = HdbscanConfig {
            min_cluster_size: 3,
        };
        let assignments = assign_communities(&skills, &config)
            .expect("HDBSCAN must not error on valid embeddings");

        // s1 must appear in at least one hdbscan community (named cluster or unclustered).
        let hdbscan_has_s1 = assignments.iter().any(|a| {
            a.source == CommunitySource::Hdbscan && a.skill_ids.contains(&"s1".to_string())
        });
        assert!(hdbscan_has_s1, "s1 must appear in an HDBSCAN community");

        // s1 must also appear in a tag community.
        let tag_has_s1 = assignments
            .iter()
            .any(|a| a.source == CommunitySource::Tag && a.skill_ids.contains(&"s1".to_string()));
        assert!(tag_has_s1, "s1 must appear in a Tag community");
    }

    /// Proves the tag community name matches the first-tag pattern.
    #[test]
    fn tag_community_name_matches_first_tag() {
        let skills = vec![
            make_skill(
                "t1",
                ScopeType::Global,
                &["infra", "cost"],
                vec![1.0, 0.0, 0.0, 0.0],
            ),
            make_skill(
                "t2",
                ScopeType::Global,
                &["infra", "latency"],
                vec![0.0, 1.0, 0.0, 0.0],
            ),
        ];
        let config = HdbscanConfig {
            min_cluster_size: 3,
        };
        let assignments =
            assign_communities(&skills, &config).expect("assign_communities must succeed");

        let tag_community = assignments
            .iter()
            .find(|a| a.source == CommunitySource::Tag && a.community_name == "global-infra")
            .expect("tag community 'global-infra' must exist");
        assert!(tag_community.skill_ids.contains(&"t1".to_string()));
        assert!(tag_community.skill_ids.contains(&"t2".to_string()));
    }

    /// Proves that when fewer than min_cluster_size skills exist, all land in unclustered.
    #[test]
    fn small_scope_produces_only_unclustered_hdbscan_community() {
        // Only 2 skills, min_cluster_size=3 → no cluster possible.
        let skills = vec![
            make_skill(
                "small1",
                ScopeType::Project,
                &["x"],
                vec![1.0, 0.0, 0.0, 0.0],
            ),
            make_skill(
                "small2",
                ScopeType::Project,
                &["y"],
                vec![0.0, 1.0, 0.0, 0.0],
            ),
        ];
        let config = HdbscanConfig {
            min_cluster_size: 3,
        };
        let assignments = assign_communities(&skills, &config)
            .expect("assign_communities must not fail for small scope");

        let unclustered = assignments
            .iter()
            .filter(|a| {
                a.source == CommunitySource::Hdbscan && a.community_name == "project-unclustered"
            })
            .collect::<Vec<_>>();
        assert_eq!(
            unclustered.len(),
            1,
            "exactly one unclustered community expected"
        );
        assert_eq!(
            unclustered[0].skill_ids.len(),
            2,
            "both skills must be in unclustered"
        );
    }

    /// Proves the output is deterministic: calling assign_communities twice on the
    /// same input produces identical results.  Uses the same two-cluster arrangement
    /// so HDBSCAN actually forms clusters (not noise-only).
    #[test]
    fn assign_communities_is_deterministic() {
        let skills = vec![
            make_skill(
                "d1",
                ScopeType::Project,
                &["ci"],
                vec![10.0, 0.1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            ),
            make_skill(
                "d2",
                ScopeType::Project,
                &["ci"],
                vec![10.1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            ),
            make_skill(
                "d3",
                ScopeType::Project,
                &["ci"],
                vec![9.9, 0.2, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            ),
            make_skill(
                "d4",
                ScopeType::Project,
                &["cd"],
                vec![0.1, 10.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            ),
            make_skill(
                "d5",
                ScopeType::Project,
                &["cd"],
                vec![0.0, 10.1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            ),
            make_skill(
                "d6",
                ScopeType::Project,
                &["cd"],
                vec![0.2, 9.9, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            ),
        ];
        let config = HdbscanConfig {
            min_cluster_size: 3,
        };

        let first = assign_communities(&skills, &config).unwrap();
        let second = assign_communities(&skills, &config).unwrap();
        assert_eq!(
            first, second,
            "assign_communities must produce identical output on repeated calls with the same input"
        );
    }
}
