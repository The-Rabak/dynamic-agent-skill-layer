use std::sync::Arc;

use admin::tools::{GraphSnapshotReader, SkillEdgeSnapshot};
use retrieval::SkillRetriever;
use serde::{Deserialize, Serialize};

use super::find_skill::{FindSkillRequest, FindSkillTool, RetrievalContext, SkillMatch};

/// Request for the `search_skill_graph` tool.
///
/// `prompt` is matched against the skill graph; `limit` caps how many skills
/// appear in `matches` (default 5). Graph neighbors and conflicts are derived
/// from the T05 `skill_edges` table for every matched skill.
#[derive(Debug, Clone, Deserialize)]
pub struct SearchSkillGraphRequest {
    pub prompt: String,
    pub limit: Option<usize>,
}

/// A single graph neighbor returned in the `neighbors` section.
///
/// Only positive-signal edge types (`depends_on`, `composes_with`, `similar_to`)
/// appear here. `conflicts_with` edges are kept in the separate `conflicts` list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillNeighbor {
    /// Stable ID of the neighboring skill.
    pub skill_id: String,
    /// Relationship type: `depends_on`, `composes_with`, or `similar_to`.
    pub edge_type: String,
    /// Direction: `"outbound"` (source→match) or `"inbound"` (match→source).
    pub direction: String,
    /// Edge origin: `cold_start_proposal`, `human_approved`, `llm_proposed`, `community_derived`.
    pub origin: String,
    /// Confidence in [0,1].
    pub confidence: f32,
    /// Human-readable reason for this edge.
    pub reason: String,
}

/// A single conflict signal returned in the `conflicts` section.
///
/// Conflict signals are deliberately kept separate from `matches` and `neighbors`
/// so agents never fold them into positive relevance scores.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillConflict {
    /// Stable ID of the conflicting skill.
    pub skill_id: String,
    /// Direction: `"outbound"` or `"inbound"`.
    pub direction: String,
    /// Edge origin.
    pub origin: String,
    /// Confidence in [0,1].
    pub confidence: f32,
    /// Human-readable reason for the conflict.
    pub reason: String,
}

/// Response from the `search_skill_graph` tool.
///
/// The three sections are always kept separate:
/// - `matches`: ranked retrieval results with relevance scores and rationale.
/// - `neighbors`: positive graph neighbors (depends_on / composes_with / similar_to).
/// - `conflicts`: conflict signals (conflicts_with) — NEVER merged into `matches`.
///
/// An agent MUST NOT add `conflicts` to its positive context injection. They are
/// surfaced here so the agent can *avoid* co-selecting conflicting skills and can
/// surface the conflict to the user when relevant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchSkillGraphResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason_code: Option<String>,
    /// Ranked skill matches with relevance scores and per-skill rationale.
    pub matches: Vec<SkillMatch>,
    /// Positive graph neighbors of the matched skills (depends_on / composes_with / similar_to).
    pub neighbors: Vec<SkillNeighbor>,
    /// Conflict signals for the matched skills (conflicts_with edges). Kept separate.
    pub conflicts: Vec<SkillConflict>,
    /// Provenance of the retrieval result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retrieval_context: Option<RetrievalContext>,
    /// Wall-clock latency for the retrieval step in milliseconds.
    pub latency_ms: u128,
}

/// Tool that exposes the SkillDAG graph surface: ranked matches, typed neighbors,
/// and conflict signals in separate sections.
///
/// Wraps `FindSkillTool` for the retrieval step, then derives neighbors and
/// conflicts from the T05 `skill_edges` table via `GraphSnapshotReader`.
#[derive(Clone)]
pub struct SearchSkillGraphTool {
    find_skill: FindSkillTool,
    graph_reader: Arc<dyn GraphSnapshotReader>,
}

impl SearchSkillGraphTool {
    /// Creates the tool with the given retriever and graph reader.
    pub fn new(
        retriever: Arc<dyn SkillRetriever>,
        graph_reader: Arc<dyn GraphSnapshotReader>,
    ) -> Self {
        Self {
            find_skill: FindSkillTool::new(retriever),
            graph_reader,
        }
    }

    /// Creates the tool with provenance context for the `retrieval_context` field (#243).
    pub fn with_provenance(
        retriever: Arc<dyn SkillRetriever>,
        graph_reader: Arc<dyn GraphSnapshotReader>,
        embedding_model: impl Into<String>,
        collection: impl Into<String>,
    ) -> Self {
        Self {
            find_skill: FindSkillTool::with_provenance(retriever, embedding_model, collection),
            graph_reader,
        }
    }

    /// Exposes the graph reader for builder-style re-wiring.
    pub fn graph_reader(&self) -> &Arc<dyn GraphSnapshotReader> {
        &self.graph_reader
    }

    pub async fn invoke(&self, request: SearchSkillGraphRequest) -> SearchSkillGraphResponse {
        let started = std::time::Instant::now();

        // Run the retrieval step via the existing FindSkillTool path.
        let find_response = self
            .find_skill
            .invoke(FindSkillRequest {
                prompt: request.prompt,
                limit: request.limit,
            })
            .await;

        if find_response.status == "degraded" {
            return SearchSkillGraphResponse {
                status: "degraded".to_owned(),
                reason_code: find_response.reason_code,
                matches: Vec::new(),
                neighbors: Vec::new(),
                conflicts: Vec::new(),
                retrieval_context: None,
                latency_ms: started.elapsed().as_millis(),
            };
        }

        // Build the set of matched skill UUIDs so edge filtering is O(1) per edge.
        // SkillMatch.skill_id carries the stable UUID threaded from ScoredSkill.skill.id.
        let matched_ids: std::collections::HashSet<&str> = find_response
            .matches
            .iter()
            .map(|m| m.skill_id.as_str())
            .collect();

        // Load typed edges. On cold start or an empty graph this is Ok(vec![]).
        // A real DB error surfaces as a degraded response — do NOT pretend there
        // are zero edges when the store is unreachable (FIX 2).
        let all_edges: Vec<SkillEdgeSnapshot> = match self.graph_reader.list_skill_edges().await {
            Ok(edges) => edges,
            Err(err) => {
                return SearchSkillGraphResponse {
                    status: "degraded".to_owned(),
                    reason_code: Some(format!("graph_edge_read_failed: {err}")),
                    matches: find_response.matches,
                    neighbors: Vec::new(),
                    conflicts: Vec::new(),
                    retrieval_context: find_response.retrieval_context,
                    latency_ms: started.elapsed().as_millis(),
                };
            }
        };

        // Filter edges to only those incident on at least one matched skill, then
        // separate positive neighbors from conflict signals.
        // Conflict signals MUST NOT enter the `neighbors` list — invariant enforced
        // and tested in `classify_edges_for_matches`.
        let (neighbors, conflicts) = classify_edges_for_matches(&all_edges, &matched_ids);

        let status = if find_response.status == "no_match" {
            "no_match"
        } else {
            "ok"
        };

        SearchSkillGraphResponse {
            status: status.to_owned(),
            reason_code: find_response.reason_code,
            matches: find_response.matches,
            neighbors,
            conflicts,
            retrieval_context: find_response.retrieval_context,
            latency_ms: started.elapsed().as_millis(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use admin::tools::SkillEdgeSnapshot;

    use super::*;

    /// Proves that `conflicts_with` edges appear in `conflicts` and NEVER in `neighbors`.
    ///
    /// This is the core separation invariant: agents must receive conflict signals
    /// as a distinct list and must not fold them into positive match scores.
    #[test]
    fn conflicts_with_edges_go_to_conflicts_not_neighbors() {
        // skill-a is a matched skill. It has two outbound edges: one positive
        // (depends_on → skill-b) and one conflict (conflicts_with → skill-c).
        let edges = vec![
            SkillEdgeSnapshot {
                source_skill_id: "skill-a".to_owned(),
                target_skill_id: "skill-b".to_owned(),
                edge_type: "depends_on".to_owned(),
                origin: "cold_start_proposal".to_owned(),
                confidence: 0.9,
                reason: "b is a prerequisite of a".to_owned(),
            },
            SkillEdgeSnapshot {
                source_skill_id: "skill-a".to_owned(),
                target_skill_id: "skill-c".to_owned(),
                edge_type: "conflicts_with".to_owned(),
                origin: "human_approved".to_owned(),
                confidence: 0.95,
                reason: "a and c must not be co-selected".to_owned(),
            },
        ];

        let matched: HashSet<&str> = ["skill-a"].into();
        let (neighbors, conflicts) = classify_edges_for_matches(&edges, &matched);

        assert_eq!(neighbors.len(), 1, "depends_on must appear in neighbors");
        assert_eq!(
            conflicts.len(),
            1,
            "conflicts_with must appear in conflicts"
        );
        assert_eq!(
            neighbors[0].edge_type, "depends_on",
            "neighbor edge_type must be preserved"
        );
        assert_eq!(
            neighbors[0].skill_id, "skill-b",
            "neighbor skill_id must be the non-matched endpoint (target)"
        );
        assert_eq!(
            neighbors[0].direction, "outbound",
            "direction must be outbound when matched skill is the edge source"
        );
        assert_eq!(
            conflicts[0].skill_id, "skill-c",
            "conflict skill_id must be the non-matched endpoint (target)"
        );
    }

    /// Proves that an edge NOT incident on any matched skill is excluded from output.
    #[test]
    fn edge_not_incident_on_matched_skill_is_excluded() {
        let edges = vec![SkillEdgeSnapshot {
            source_skill_id: "unrelated-x".to_owned(),
            target_skill_id: "unrelated-y".to_owned(),
            edge_type: "depends_on".to_owned(),
            origin: "cold_start_proposal".to_owned(),
            confidence: 0.8,
            reason: "unrelated".to_owned(),
        }];

        // matched_ids contains neither endpoint.
        let matched: HashSet<&str> = ["skill-a"].into();
        let (neighbors, conflicts) = classify_edges_for_matches(&edges, &matched);

        assert!(
            neighbors.is_empty(),
            "edges not incident on a matched skill must be excluded from neighbors"
        );
        assert!(
            conflicts.is_empty(),
            "edges not incident on a matched skill must be excluded from conflicts"
        );
    }

    /// Proves that an inbound edge (matched skill is the target) uses direction `"inbound"`
    /// and names the source as the neighbor skill_id.
    #[test]
    fn inbound_edge_uses_inbound_direction_and_source_as_neighbor_id() {
        let edges = vec![SkillEdgeSnapshot {
            source_skill_id: "skill-upstream".to_owned(),
            target_skill_id: "skill-matched".to_owned(),
            edge_type: "depends_on".to_owned(),
            origin: "cold_start_proposal".to_owned(),
            confidence: 0.7,
            reason: "upstream depends on matched".to_owned(),
        }];

        let matched: HashSet<&str> = ["skill-matched"].into();
        let (neighbors, conflicts) = classify_edges_for_matches(&edges, &matched);

        assert_eq!(
            neighbors.len(),
            1,
            "one inbound neighbor edge must be present"
        );
        assert!(conflicts.is_empty(), "no conflict edges in input");
        assert_eq!(
            neighbors[0].direction, "inbound",
            "direction must be inbound when matched skill is the edge target"
        );
        assert_eq!(
            neighbors[0].skill_id, "skill-upstream",
            "neighbor skill_id must be the non-matched endpoint (source)"
        );
    }

    /// Verifies that all three positive edge types end up in `neighbors`.
    #[test]
    fn all_positive_edge_types_go_to_neighbors() {
        let edges = vec![
            SkillEdgeSnapshot {
                source_skill_id: "s".to_owned(),
                target_skill_id: "t1".to_owned(),
                edge_type: "depends_on".to_owned(),
                origin: "cold_start_proposal".to_owned(),
                confidence: 0.9,
                reason: "r".to_owned(),
            },
            SkillEdgeSnapshot {
                source_skill_id: "s".to_owned(),
                target_skill_id: "t2".to_owned(),
                edge_type: "composes_with".to_owned(),
                origin: "cold_start_proposal".to_owned(),
                confidence: 0.8,
                reason: "r".to_owned(),
            },
            SkillEdgeSnapshot {
                source_skill_id: "s".to_owned(),
                target_skill_id: "t3".to_owned(),
                edge_type: "similar_to".to_owned(),
                origin: "cold_start_proposal".to_owned(),
                confidence: 0.7,
                reason: "r".to_owned(),
            },
        ];
        let matched: HashSet<&str> = ["s"].into();
        let (neighbors, conflicts) = classify_edges_for_matches(&edges, &matched);
        assert_eq!(
            neighbors.len(),
            3,
            "all 3 positive types must go to neighbors"
        );
        assert_eq!(conflicts.len(), 0, "no conflicts_with edges in input");
    }
}

/// Filters edges to those incident on at least one matched skill, then separates
/// positive neighbors from conflict signals.
///
/// For each edge where `source_skill_id` is in `matched_ids`:
/// - `direction` is `"outbound"` and the neighbor/conflict `skill_id` is `target_skill_id`.
///
/// For each edge where `target_skill_id` is in `matched_ids` (and source is not):
/// - `direction` is `"inbound"` and the neighbor/conflict `skill_id` is `source_skill_id`.
///
/// An edge where BOTH endpoints are matched skills is emitted once per matched
/// endpoint (outbound from the source match and inbound to the target match) so
/// both matched skills see their adjacency.
///
/// `conflicts_with` edges go to `conflicts`; `depends_on`, `composes_with`, and
/// `similar_to` go to `neighbors`. Unknown edge types are skipped (forward-compat
/// with future migrations).
///
/// This function MUST NOT fold `conflicts_with` edges into `neighbors`.
pub(crate) fn classify_edges_for_matches(
    edges: &[SkillEdgeSnapshot],
    matched_ids: &std::collections::HashSet<&str>,
) -> (Vec<SkillNeighbor>, Vec<SkillConflict>) {
    let mut neighbors = Vec::new();
    let mut conflicts = Vec::new();

    for edge in edges {
        let source_matched = matched_ids.contains(edge.source_skill_id.as_str());
        let target_matched = matched_ids.contains(edge.target_skill_id.as_str());

        if !source_matched && !target_matched {
            continue;
        }

        // Emit once for the source-is-matched (outbound) perspective.
        if source_matched {
            emit_edge(
                edge,
                "outbound",
                &edge.target_skill_id,
                &mut neighbors,
                &mut conflicts,
            );
        }

        // Emit once for the target-is-matched (inbound) perspective, but only
        // when the source is not also matched (avoids a duplicate self-loop entry
        // for edges that connect two matched skills via the outbound pass above).
        if target_matched && !source_matched {
            emit_edge(
                edge,
                "inbound",
                &edge.source_skill_id,
                &mut neighbors,
                &mut conflicts,
            );
        }
    }

    (neighbors, conflicts)
}

/// Appends a single classified edge entry for the given perspective.
///
/// `direction` is `"outbound"` when the matched skill is the edge source,
/// `"inbound"` when it is the target. `other_endpoint` is the non-matched
/// endpoint that the resulting neighbor/conflict entry should name.
fn emit_edge(
    edge: &SkillEdgeSnapshot,
    direction: &str,
    other_endpoint: &str,
    neighbors: &mut Vec<SkillNeighbor>,
    conflicts: &mut Vec<SkillConflict>,
) {
    match edge.edge_type.as_str() {
        "depends_on" | "composes_with" | "similar_to" => {
            neighbors.push(SkillNeighbor {
                skill_id: other_endpoint.to_owned(),
                edge_type: edge.edge_type.clone(),
                direction: direction.to_owned(),
                origin: edge.origin.clone(),
                confidence: edge.confidence,
                reason: edge.reason.clone(),
            });
        }
        "conflicts_with" => {
            conflicts.push(SkillConflict {
                skill_id: other_endpoint.to_owned(),
                direction: direction.to_owned(),
                origin: edge.origin.clone(),
                confidence: edge.confidence,
                reason: edge.reason.clone(),
            });
        }
        _ => {}
    }
}
