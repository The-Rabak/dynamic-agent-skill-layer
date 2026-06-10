//! Typed skill-graph edge construction (V1.7 Slice 5 / T05).
//!
//! This module turns the structured multi-view fields already carried on each
//! [`BuiltSkill`] (`requires`, `produces`, `tools`, `artifacts`) into typed
//! inter-skill edges. The graph is SEPARATE evidence, never a ranking multiplier
//! (parent plan Design Decision #4 / the #208 lesson): edges are stored so later
//! retrieval can expose neighbours and conflicts, not so they can boost a score.
//!
//! Two deterministic cold-start rules are implemented, both sourced purely from
//! structured fields with no external API:
//!
//! 1. `depends_on` — when a skill's `requires` overlaps another skill's `produces`,
//!    the first depends on the second. A genuinely mutual overlap (A↔B) is NOT a
//!    backbone dependency; it is demoted to a single symmetric `composes_with` edge
//!    so the directed-acyclic backbone is preserved without silently dropping signal.
//! 2. `similar_to` — when two skills share enough `tools`/`artifacts` (Jaccard ≥
//!    [`SIMILAR_TO_JACCARD_FLOOR`]) they are proposed as similar.
//!
//! `specializes` and `conflicts_with` are valid stored/walkable-classified edge
//! types (see [`domain::EdgeType`]) but have no reliable deterministic structured
//! signal yet, so T05 does not auto-propose them; they are reserved for richer
//! signals / agent classification in T06+.
//!
//! Confidence drives the trust boundary: edges at or above
//! [`AUTO_COMMIT_CONFIDENCE_THRESHOLD`] are committed as trusted
//! (`cold_start_deterministic`); below it they are persisted as observable
//! proposals (`cold_start_proposal`).

use std::collections::{BTreeMap, BTreeSet};

use domain::{EdgeOrigin, EdgeType};
use serde_json::json;
use thiserror::Error;

use crate::graph::build::BuiltSkill;

/// Confidence at or above which a deterministic cold-start edge is auto-committed as
/// trusted graph state. Below it, the edge is persisted as an observable proposal.
///
/// A single exact `requires`↔`produces` token match yields exactly this confidence,
/// so unambiguous dependency evidence is auto-committed while weaker similarity
/// signals remain proposals (owner decision 2026-06-10).
pub const AUTO_COMMIT_CONFIDENCE_THRESHOLD: f32 = 0.9;

/// Jaccard floor over the union of `tools` and `artifacts` for proposing a
/// `similar_to` edge. Below this the lexical overlap is too weak to be useful.
const SIMILAR_TO_JACCARD_FLOOR: f32 = 0.5;

/// Confidence assigned to a `composes_with` edge demoted from a mutual `depends_on`.
/// Below the auto-commit threshold because a mutual overlap is inherently ambiguous
/// and should be reviewed rather than trusted automatically.
const COMPOSES_WITH_CONFIDENCE: f32 = 0.8;

/// A cold-start typed edge proposal between two skills.
///
/// `source_skill_id` / `target_skill_id` are the stable `BuiltSkill::id` values
/// (blake3 hex of the source path); the persistence layer maps them to the durable
/// `skills.id` UUIDs. `evidence` carries the structured field values that justify the
/// edge so the proposal is auditable without re-deriving it.
#[derive(Debug, Clone, PartialEq)]
pub struct ProposedSkillEdge {
    pub source_skill_id: String,
    pub target_skill_id: String,
    pub edge_type: EdgeType,
    pub origin: EdgeOrigin,
    pub confidence: f32,
    pub reason: String,
    pub evidence: serde_json::Value,
}

/// Failure raised when an edge set violates the typed-graph structural contract.
///
/// Returned by [`validate_backbone_acyclic`]; surfaced by
/// [`build_validated_cold_start_edges`] so a contradictory edge set fails loudly
/// at rebuild time rather than silently corrupting the graph.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum EdgeValidationError {
    #[error("backbone {edge_type} edges contain a cycle: {cycle}")]
    BackboneCycle { edge_type: String, cycle: String },
    #[error("self-loop edge is not allowed: {skill_id} --{edge_type}--> itself")]
    SelfLoop { skill_id: String, edge_type: String },
}

/// Generates deterministic cold-start edges and validates the backbone is acyclic.
///
/// This is the entry point the rebuild write path calls. It fails loudly if the
/// generated edges form a backbone cycle or contain a self-loop, rather than
/// committing a contradictory graph.
pub fn build_validated_cold_start_edges(
    skills: &[BuiltSkill],
) -> Result<Vec<ProposedSkillEdge>, EdgeValidationError> {
    let edges = propose_cold_start_edges(skills);
    validate_backbone_acyclic(&edges)?;
    Ok(edges)
}

/// Generates deterministic cold-start edge proposals from structured skill fields.
///
/// Pure and order-stable: the input skill order is preserved and pairs are visited
/// deterministically, so the same corpus always yields the same edges. Does not
/// validate the backbone — callers that commit edges must run
/// [`validate_backbone_acyclic`] (or use [`build_validated_cold_start_edges`]).
pub fn propose_cold_start_edges(skills: &[BuiltSkill]) -> Vec<ProposedSkillEdge> {
    let mut edges = Vec::new();
    edges.extend(propose_dependency_edges(skills));
    edges.extend(propose_similarity_edges(skills));
    edges
}

/// Validates that the backbone edge types (`depends_on`, `specializes`) form a
/// directed-acyclic graph and contain no self-loops.
///
/// Non-backbone edges (`composes_with`, `similar_to`, `conflicts_with`) are allowed
/// to form cycles and are ignored here, except that NO edge type may be a self-loop.
pub fn validate_backbone_acyclic(edges: &[ProposedSkillEdge]) -> Result<(), EdgeValidationError> {
    for edge in edges {
        if edge.source_skill_id == edge.target_skill_id {
            return Err(EdgeValidationError::SelfLoop {
                skill_id: edge.source_skill_id.clone(),
                edge_type: edge.edge_type.as_db_str().to_owned(),
            });
        }
    }

    // Cycle-check each backbone relation independently: a depends_on cycle and a
    // specializes cycle are distinct contradictions, and mixing the two adjacency
    // sets could mask or invent a cycle that neither relation actually has.
    for backbone in [EdgeType::DependsOn, EdgeType::Specializes] {
        let adjacency = backbone_adjacency(edges, backbone);
        if let Some(cycle) = first_cycle(&adjacency) {
            return Err(EdgeValidationError::BackboneCycle {
                edge_type: backbone.as_db_str().to_owned(),
                cycle: cycle.join(" -> "),
            });
        }
    }
    Ok(())
}

/// Builds a directed adjacency list for a single backbone edge type, with stable
/// (sorted) ordering so cycle detection is deterministic.
fn backbone_adjacency(
    edges: &[ProposedSkillEdge],
    edge_type: EdgeType,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut adjacency: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for edge in edges.iter().filter(|edge| edge.edge_type == edge_type) {
        adjacency
            .entry(edge.source_skill_id.clone())
            .or_default()
            .insert(edge.target_skill_id.clone());
    }
    adjacency
}

/// Returns the first detected cycle as an ordered node path (closing back to the
/// repeated node), or `None` if the graph is acyclic. Iterative DFS with a
/// white/gray/black colouring so a back-edge into a gray node reveals the cycle.
fn first_cycle(adjacency: &BTreeMap<String, BTreeSet<String>>) -> Option<Vec<String>> {
    #[derive(Clone, Copy, PartialEq)]
    enum Mark {
        Gray,
        Black,
    }

    let mut state: BTreeMap<String, Mark> = BTreeMap::new();

    for root in adjacency.keys() {
        if state.contains_key(root) {
            continue;
        }
        // Explicit stack of (node, path-so-far) to avoid deep recursion on long chains.
        let mut stack: Vec<(String, Vec<String>)> = vec![(root.clone(), vec![root.clone()])];
        state.insert(root.clone(), Mark::Gray);

        while let Some((node, path)) = stack.pop() {
            let mut fully_explored = true;
            if let Some(targets) = adjacency.get(&node) {
                for target in targets {
                    match state.get(target) {
                        Some(Mark::Gray) => {
                            // Back-edge into an in-progress node: reconstruct the cycle.
                            let mut cycle = path.clone();
                            cycle.push(target.clone());
                            return Some(cycle);
                        }
                        Some(Mark::Black) => {}
                        None => {
                            fully_explored = false;
                            state.insert(target.clone(), Mark::Gray);
                            let mut next_path = path.clone();
                            next_path.push(target.clone());
                            // Re-push the current node so it is marked black only after
                            // all of its descendants have been explored.
                            stack.push((node.clone(), path.clone()));
                            stack.push((target.clone(), next_path));
                            break;
                        }
                    }
                }
            }
            if fully_explored {
                state.insert(node, Mark::Black);
            }
        }
    }
    None
}

/// Proposes `depends_on` edges from `requires`↔`produces` overlap, demoting any
/// mutual overlap to a single symmetric `composes_with` edge to keep the backbone
/// acyclic.
fn propose_dependency_edges(skills: &[BuiltSkill]) -> Vec<ProposedSkillEdge> {
    // First pass: record directional overlaps keyed by ordered (source, target) id.
    let mut directional: BTreeMap<(usize, usize), Vec<String>> = BTreeMap::new();
    for (source_idx, source) in skills.iter().enumerate() {
        let source_requires = normalized_set(&source.requires);
        if source_requires.is_empty() {
            continue;
        }
        for (target_idx, target) in skills.iter().enumerate() {
            if source_idx == target_idx {
                continue;
            }
            let overlap = sorted_intersection(&source_requires, &normalized_set(&target.produces));
            if !overlap.is_empty() {
                directional.insert((source_idx, target_idx), overlap);
            }
        }
    }

    let mut edges = Vec::new();
    let mut consumed_pairs: BTreeSet<(usize, usize)> = BTreeSet::new();

    for (&(source_idx, target_idx), forward_overlap) in &directional {
        let unordered = (source_idx.min(target_idx), source_idx.max(target_idx));
        if consumed_pairs.contains(&unordered) {
            continue;
        }

        if directional.contains_key(&(target_idx, source_idx)) {
            // Mutual requires↔produces: ambiguous backbone direction. Demote to one
            // symmetric composes_with edge (canonical lower id as source) instead of
            // emitting a 2-cycle of depends_on. Observable via reason + evidence.
            consumed_pairs.insert(unordered);
            let (low, high) = unordered;
            edges.push(ProposedSkillEdge {
                source_skill_id: skills[low].id.clone(),
                target_skill_id: skills[high].id.clone(),
                edge_type: EdgeType::ComposesWith,
                origin: origin_for(COMPOSES_WITH_CONFIDENCE),
                confidence: COMPOSES_WITH_CONFIDENCE,
                reason: format!(
                    "mutual requires↔produces overlap between '{}' and '{}' — demoted from \
                     depends_on to composes_with to preserve backbone acyclicity",
                    skills[low].name, skills[high].name
                ),
                evidence: json!({
                    "rule": "mutual_requires_produces",
                    "forward_overlap": directional.get(&(low, high)),
                    "reverse_overlap": directional.get(&(high, low)),
                }),
            });
        } else {
            let confidence = dependency_confidence(forward_overlap.len());
            edges.push(ProposedSkillEdge {
                source_skill_id: skills[source_idx].id.clone(),
                target_skill_id: skills[target_idx].id.clone(),
                edge_type: EdgeType::DependsOn,
                origin: origin_for(confidence),
                confidence,
                reason: format!(
                    "'{}' requires what '{}' produces: {}",
                    skills[source_idx].name,
                    skills[target_idx].name,
                    forward_overlap.join(", ")
                ),
                evidence: json!({
                    "rule": "requires_produces_overlap",
                    "source_requires": normalized_vec(&skills[source_idx].requires),
                    "target_produces": normalized_vec(&skills[target_idx].produces),
                    "overlap": forward_overlap,
                }),
            });
        }
    }
    edges
}

/// Proposes symmetric `similar_to` edges from shared `tools`/`artifacts` (Jaccard).
/// Emits one canonical edge per pair (lower index as source) to avoid duplicates.
fn propose_similarity_edges(skills: &[BuiltSkill]) -> Vec<ProposedSkillEdge> {
    let mut edges = Vec::new();
    for low in 0..skills.len() {
        let low_terms = lexical_terms(&skills[low]);
        if low_terms.is_empty() {
            continue;
        }
        for high in (low + 1)..skills.len() {
            let high_terms = lexical_terms(&skills[high]);
            if high_terms.is_empty() {
                continue;
            }
            let shared = sorted_intersection(&low_terms, &high_terms);
            if shared.is_empty() {
                continue;
            }
            let union_size = low_terms.union(&high_terms).count();
            let jaccard = shared.len() as f32 / union_size as f32;
            if jaccard < SIMILAR_TO_JACCARD_FLOOR {
                continue;
            }
            edges.push(ProposedSkillEdge {
                source_skill_id: skills[low].id.clone(),
                target_skill_id: skills[high].id.clone(),
                edge_type: EdgeType::SimilarTo,
                origin: origin_for(jaccard),
                confidence: jaccard,
                reason: format!(
                    "'{}' and '{}' share tools/artifacts: {}",
                    skills[low].name,
                    skills[high].name,
                    shared.join(", ")
                ),
                evidence: json!({
                    "rule": "tools_artifacts_jaccard",
                    "jaccard": jaccard,
                    "shared": shared,
                }),
            });
        }
    }
    edges
}

/// Maps a confidence to an origin using the auto-commit threshold.
fn origin_for(confidence: f32) -> EdgeOrigin {
    if confidence >= AUTO_COMMIT_CONFIDENCE_THRESHOLD {
        EdgeOrigin::ColdStartDeterministic
    } else {
        EdgeOrigin::ColdStartProposal
    }
}

/// Confidence for a `depends_on` edge given the number of overlapping terms.
/// A single exact match is auto-commit-worthy; more overlap only raises confidence.
fn dependency_confidence(overlap_len: usize) -> f32 {
    (AUTO_COMMIT_CONFIDENCE_THRESHOLD + 0.02 * (overlap_len.saturating_sub(1)) as f32).min(1.0)
}

/// The lexical term set used for `similar_to`: the union of normalized `tools` and
/// `artifacts`. These are the high-signal exact-string fields the dense embedding
/// blurs, so they are the right basis for a lexical similarity edge.
fn lexical_terms(skill: &BuiltSkill) -> BTreeSet<String> {
    let mut terms = normalized_set(&skill.tools);
    terms.extend(normalized_set(&skill.artifacts));
    terms
}

/// Lowercases and trims each value, dropping blanks, into a deduplicated set.
fn normalized_set(values: &[String]) -> BTreeSet<String> {
    values
        .iter()
        .map(|value| value.trim().to_lowercase())
        .filter(|value| !value.is_empty())
        .collect()
}

/// Normalized values as a stable sorted vec (for evidence payloads).
fn normalized_vec(values: &[String]) -> Vec<String> {
    normalized_set(values).into_iter().collect()
}

/// Sorted intersection of two normalized sets.
fn sorted_intersection(left: &BTreeSet<String>, right: &BTreeSet<String>) -> Vec<String> {
    left.intersection(right).cloned().collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use domain::ScopeType;

    use super::*;

    /// Builds a minimal `BuiltSkill` with only the structured fields the edge rules
    /// read; everything else is empty/default since edges never touch it.
    fn skill(id: &str, name: &str) -> BuiltSkill {
        BuiltSkill {
            id: id.to_owned(),
            scope_id: "project".to_owned(),
            scope_type: ScopeType::Project,
            source_path: PathBuf::from(format!("/tmp/{id}/SKILL.md")),
            name: name.to_owned(),
            description: String::new(),
            tags: Vec::new(),
            subunits: Vec::new(),
            embedding: Vec::new(),
            use_when: Vec::new(),
            avoid_when: Vec::new(),
            artifacts: Vec::new(),
            tools: Vec::new(),
            invariants: Vec::new(),
            requires: Vec::new(),
            produces: Vec::new(),
        }
    }

    #[test]
    fn requires_produces_overlap_proposes_auto_committed_depends_on() {
        let mut consumer = skill("a", "deploy-service");
        consumer.requires = vec!["Compiled Binary".to_owned()];
        let mut producer = skill("b", "build-binary");
        producer.produces = vec!["compiled binary".to_owned()];

        let edges = propose_cold_start_edges(&[consumer, producer]);

        let depends = edges
            .iter()
            .find(|edge| edge.edge_type == EdgeType::DependsOn)
            .expect("a depends_on edge must be proposed from requires↔produces overlap");
        assert_eq!(depends.source_skill_id, "a", "consumer is the dependent");
        assert_eq!(depends.target_skill_id, "b", "producer is the dependency");
        assert_eq!(
            depends.origin,
            EdgeOrigin::ColdStartDeterministic,
            "an exact prerequisite match is high-confidence and auto-committed"
        );
        assert!(depends.confidence >= AUTO_COMMIT_CONFIDENCE_THRESHOLD);
        assert_eq!(depends.evidence["overlap"][0], "compiled binary");
    }

    #[test]
    fn no_overlap_proposes_no_dependency_edge() {
        let mut a = skill("a", "alpha");
        a.requires = vec!["postgres".to_owned()];
        let mut b = skill("b", "beta");
        b.produces = vec!["redis stream".to_owned()];

        let edges = propose_cold_start_edges(&[a, b]);

        assert!(
            !edges.iter().any(|edge| edge.edge_type == EdgeType::DependsOn),
            "disjoint requires/produces must not invent a dependency"
        );
    }

    #[test]
    fn mutual_overlap_is_demoted_to_composes_with_not_a_backbone_cycle() {
        let mut a = skill("a", "alpha");
        a.requires = vec!["thing-b".to_owned()];
        a.produces = vec!["thing-a".to_owned()];
        let mut b = skill("b", "beta");
        b.requires = vec!["thing-a".to_owned()];
        b.produces = vec!["thing-b".to_owned()];

        let edges = build_validated_cold_start_edges(&[a, b])
            .expect("mutual overlap must not produce a backbone cycle");

        assert!(
            !edges.iter().any(|edge| edge.edge_type == EdgeType::DependsOn),
            "mutual overlap must not emit depends_on in either direction"
        );
        let composes = edges
            .iter()
            .find(|edge| edge.edge_type == EdgeType::ComposesWith)
            .expect("mutual overlap must be demoted to a single composes_with edge");
        assert_eq!(composes.source_skill_id, "a");
        assert_eq!(composes.target_skill_id, "b");
        assert_eq!(composes.origin, EdgeOrigin::ColdStartProposal);
    }

    #[test]
    fn shared_tools_propose_similar_to_proposal_edge() {
        let mut a = skill("a", "alpha");
        a.tools = vec!["qdrant".to_owned(), "ollama".to_owned()];
        let mut b = skill("b", "beta");
        b.tools = vec!["qdrant".to_owned(), "ollama".to_owned()];

        let edges = propose_cold_start_edges(&[a, b]);

        let similar = edges
            .iter()
            .find(|edge| edge.edge_type == EdgeType::SimilarTo)
            .expect("fully shared tool sets must propose similar_to");
        assert_eq!(similar.source_skill_id, "a");
        assert_eq!(similar.target_skill_id, "b");
        // Jaccard 1.0 → auto-committed; identical tool sets are unambiguous.
        assert_eq!(similar.origin, EdgeOrigin::ColdStartDeterministic);
    }

    #[test]
    fn weak_tool_overlap_below_floor_proposes_no_similar_to() {
        let mut a = skill("a", "alpha");
        a.tools = vec![
            "qdrant".to_owned(),
            "ollama".to_owned(),
            "redis".to_owned(),
        ];
        let mut b = skill("b", "beta");
        b.tools = vec!["qdrant".to_owned(), "kafka".to_owned(), "nats".to_owned()];

        let edges = propose_cold_start_edges(&[a, b]);

        assert!(
            !edges.iter().any(|edge| edge.edge_type == EdgeType::SimilarTo),
            "1/5 Jaccard is below the floor and must not propose similar_to"
        );
    }

    #[test]
    fn validate_rejects_a_hand_built_backbone_cycle() {
        let cyclic = vec![
            ProposedSkillEdge {
                source_skill_id: "a".to_owned(),
                target_skill_id: "b".to_owned(),
                edge_type: EdgeType::DependsOn,
                origin: EdgeOrigin::Manual,
                confidence: 1.0,
                reason: String::new(),
                evidence: json!({}),
            },
            ProposedSkillEdge {
                source_skill_id: "b".to_owned(),
                target_skill_id: "a".to_owned(),
                edge_type: EdgeType::DependsOn,
                origin: EdgeOrigin::Manual,
                confidence: 1.0,
                reason: String::new(),
                evidence: json!({}),
            },
        ];

        let err = validate_backbone_acyclic(&cyclic)
            .expect_err("a depends_on 2-cycle must fail clearly");
        assert!(matches!(err, EdgeValidationError::BackboneCycle { .. }));
    }

    #[test]
    fn validate_rejects_self_loop() {
        let self_loop = vec![ProposedSkillEdge {
            source_skill_id: "a".to_owned(),
            target_skill_id: "a".to_owned(),
            edge_type: EdgeType::SimilarTo,
            origin: EdgeOrigin::Manual,
            confidence: 1.0,
            reason: String::new(),
            evidence: json!({}),
        }];

        let err =
            validate_backbone_acyclic(&self_loop).expect_err("a self-loop must fail clearly");
        assert!(matches!(err, EdgeValidationError::SelfLoop { .. }));
    }

    #[test]
    fn validate_allows_non_backbone_cycles() {
        // similar_to is symmetric/walkable but NOT backbone, so a 2-cycle is fine.
        let similar_cycle = vec![
            ProposedSkillEdge {
                source_skill_id: "a".to_owned(),
                target_skill_id: "b".to_owned(),
                edge_type: EdgeType::SimilarTo,
                origin: EdgeOrigin::Manual,
                confidence: 1.0,
                reason: String::new(),
                evidence: json!({}),
            },
            ProposedSkillEdge {
                source_skill_id: "b".to_owned(),
                target_skill_id: "a".to_owned(),
                edge_type: EdgeType::SimilarTo,
                origin: EdgeOrigin::Manual,
                confidence: 1.0,
                reason: String::new(),
                evidence: json!({}),
            },
        ];

        assert!(
            validate_backbone_acyclic(&similar_cycle).is_ok(),
            "non-backbone cycles are permitted"
        );
    }

    #[test]
    fn validate_detects_longer_backbone_chain_cycle() {
        let chain = ["a", "b", "c", "a"];
        let edges: Vec<ProposedSkillEdge> = chain
            .windows(2)
            .map(|pair| ProposedSkillEdge {
                source_skill_id: pair[0].to_owned(),
                target_skill_id: pair[1].to_owned(),
                edge_type: EdgeType::Specializes,
                origin: EdgeOrigin::Manual,
                confidence: 1.0,
                reason: String::new(),
                evidence: json!({}),
            })
            .collect();

        let err = validate_backbone_acyclic(&edges)
            .expect_err("a 3-node specializes cycle must fail clearly");
        match err {
            EdgeValidationError::BackboneCycle { edge_type, .. } => {
                assert_eq!(edge_type, "specializes");
            }
            other => panic!("expected a backbone cycle error, got {other:?}"),
        }
    }
}
