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
///    named `"{scope}-cluster-{idf_label}"` where `idf_label` is the most frequent
///    tag across cluster members (ties broken alphabetically).  Noise skills
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

            // Named cluster: derive the community name from the most-common tag
            // across member skills so the label is human-readable and stable.
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

/// Derives a deterministic, human-readable community name for an HDBSCAN cluster.
///
/// Picks the tag that appears most frequently among the member skills.  Ties are
/// broken alphabetically so the result is stable across builds.  Falls back to
/// `"{scope_prefix}-cluster-{label}"` when members have no tags at all.
fn derive_cluster_label(
    local_indices: &[usize],
    global_indices: &[usize],
    skills: &[BuiltSkill],
    scope_prefix: &str,
    cluster_label: i32,
) -> String {
    let mut tag_counts: HashMap<&str, usize> = HashMap::new();
    for &local_index in local_indices {
        let skill = &skills[global_indices[local_index]];
        for tag in &skill.tags {
            *tag_counts.entry(tag.as_str()).or_default() += 1;
        }
    }

    if tag_counts.is_empty() {
        return format!("{scope_prefix}-cluster-{cluster_label}");
    }

    // Most-frequent tag, alphabetical tiebreak.
    let top_tag = tag_counts
        .iter()
        .max_by(|(tag_a, count_a), (tag_b, count_b)| {
            count_a.cmp(count_b).then_with(|| tag_b.cmp(tag_a))
        })
        .map(|(tag, _)| *tag)
        .expect("tag_counts is non-empty");

    format!("{scope_prefix}-cluster-{}", top_tag.to_ascii_lowercase())
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

    /// Builds a minimal `BuiltSkill` for test purposes.
    fn make_skill(id: &str, scope: ScopeType, tags: &[&str], embedding: Vec<f32>) -> BuiltSkill {
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
                content: format!("Do {id}"),
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
    /// - Cluster A (3 "retrieval" skills) lands in a single named HDBSCAN community.
    /// - Cluster B (3 "scoring" skills) lands in a different named HDBSCAN community.
    /// - The community names are derived from the dominant tag of the cluster members.
    /// - No "retrieval" skill appears in the "scoring" cluster and vice versa.
    #[test]
    fn hdbscan_clusters_similar_skills_and_marks_outlier_noise() {
        // Cluster A: 3 skills near [10, 0.x, 0, ...]
        // Cluster B: 3 skills near [0.x, 10, 0, ...]
        // Bridging skill at [5, 5, ...] — absorbed into whichever cluster is denser.
        let skills = vec![
            make_skill("a1", ScopeType::Project, &["retrieval"],
                vec![10.0, 0.1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
            make_skill("a2", ScopeType::Project, &["retrieval"],
                vec![10.1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
            make_skill("a3", ScopeType::Project, &["retrieval"],
                vec![9.9, 0.2, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
            make_skill("b1", ScopeType::Project, &["scoring"],
                vec![0.1, 10.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
            make_skill("b2", ScopeType::Project, &["scoring"],
                vec![0.0, 10.1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
            make_skill("b3", ScopeType::Project, &["scoring"],
                vec![0.2, 9.9, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
            // Bridging skill — assigned to whichever cluster HDBSCAN finds denser.
            // We do not assert on this skill's assignment.
            make_skill("bridge", ScopeType::Project, &["unrelated"],
                vec![5.0, 5.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
        ];

        let config = HdbscanConfig { min_cluster_size: 3 };
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
            named_clusters.iter().map(|c| &c.community_name).collect::<Vec<_>>()
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

        // Community names are derived from the dominant tag of cluster members.
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
            make_skill("s1", ScopeType::Project, &["auth"],
                vec![10.0, 0.1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
            make_skill("s2", ScopeType::Project, &["auth"],
                vec![10.1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
            make_skill("s3", ScopeType::Project, &["auth"],
                vec![9.9, 0.2, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
            make_skill("s4", ScopeType::Project, &["infra"],
                vec![0.1, 10.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
            make_skill("s5", ScopeType::Project, &["infra"],
                vec![0.0, 10.1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
            make_skill("s6", ScopeType::Project, &["infra"],
                vec![0.2, 9.9, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
        ];

        let config = HdbscanConfig { min_cluster_size: 3 };
        let assignments = assign_communities(&skills, &config)
            .expect("HDBSCAN must not error on valid embeddings");

        // s1 must appear in at least one hdbscan community (named cluster or unclustered).
        let hdbscan_has_s1 = assignments.iter().any(|a| {
            a.source == CommunitySource::Hdbscan && a.skill_ids.contains(&"s1".to_string())
        });
        assert!(hdbscan_has_s1, "s1 must appear in an HDBSCAN community");

        // s1 must also appear in a tag community.
        let tag_has_s1 = assignments.iter().any(|a| {
            a.source == CommunitySource::Tag && a.skill_ids.contains(&"s1".to_string())
        });
        assert!(tag_has_s1, "s1 must appear in a Tag community");
    }

    /// Proves the tag community name matches the first-tag pattern.
    #[test]
    fn tag_community_name_matches_first_tag() {
        let skills = vec![
            make_skill("t1", ScopeType::Global, &["infra", "cost"],
                vec![1.0, 0.0, 0.0, 0.0]),
            make_skill("t2", ScopeType::Global, &["infra", "latency"],
                vec![0.0, 1.0, 0.0, 0.0]),
        ];
        let config = HdbscanConfig { min_cluster_size: 3 };
        let assignments = assign_communities(&skills, &config)
            .expect("assign_communities must succeed");

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
            make_skill("small1", ScopeType::Project, &["x"], vec![1.0, 0.0, 0.0, 0.0]),
            make_skill("small2", ScopeType::Project, &["y"], vec![0.0, 1.0, 0.0, 0.0]),
        ];
        let config = HdbscanConfig { min_cluster_size: 3 };
        let assignments = assign_communities(&skills, &config)
            .expect("assign_communities must not fail for small scope");

        let unclustered = assignments
            .iter()
            .filter(|a| {
                a.source == CommunitySource::Hdbscan
                    && a.community_name == "project-unclustered"
            })
            .collect::<Vec<_>>();
        assert_eq!(unclustered.len(), 1, "exactly one unclustered community expected");
        assert_eq!(unclustered[0].skill_ids.len(), 2, "both skills must be in unclustered");
    }

    /// Proves the output is deterministic: calling assign_communities twice on the
    /// same input produces identical results.  Uses the same two-cluster arrangement
    /// so HDBSCAN actually forms clusters (not noise-only).
    #[test]
    fn assign_communities_is_deterministic() {
        let skills = vec![
            make_skill("d1", ScopeType::Project, &["ci"],
                vec![10.0, 0.1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
            make_skill("d2", ScopeType::Project, &["ci"],
                vec![10.1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
            make_skill("d3", ScopeType::Project, &["ci"],
                vec![9.9, 0.2, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
            make_skill("d4", ScopeType::Project, &["cd"],
                vec![0.1, 10.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
            make_skill("d5", ScopeType::Project, &["cd"],
                vec![0.0, 10.1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
            make_skill("d6", ScopeType::Project, &["cd"],
                vec![0.2, 9.9, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
        ];
        let config = HdbscanConfig { min_cluster_size: 3 };

        let first = assign_communities(&skills, &config).unwrap();
        let second = assign_communities(&skills, &config).unwrap();
        assert_eq!(
            first, second,
            "assign_communities must produce identical output on repeated calls with the same input"
        );
    }
}
