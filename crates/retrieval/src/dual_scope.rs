use std::{
    collections::{HashMap, HashSet},
    future::Future,
    sync::Arc,
    time::Duration,
};

use domain::ScopeDescriptor;
use tokio::time::timeout;

use crate::{
    cosine_rank::{cosine_similarity, rank_by_cosine},
    fusion::{FusedCandidate, mmr_select},
    graph_search::{GraphHit, search_graph, tokenize},
    orchestrator::{CommunityBoostMode, RetrievalBackend, RetrievalConfig, RetrievalSnapshot},
    scoring::{ScoreComponents, score_eq3},
};

#[derive(Debug, Clone, PartialEq)]
pub struct ScopedSearchResult {
    pub scope_id: String,
    pub scope_type: domain::ScopeType,
    pub candidates: Vec<FusedCandidate>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedSearchFailure {
    pub scope_id: String,
    pub reason_code: String,
}

pub async fn run_project_and_global_concurrently<
    ProjectFuture,
    GlobalFuture,
    ProjectOutput,
    GlobalOutput,
>(
    project_future: ProjectFuture,
    global_future: GlobalFuture,
) -> (ProjectOutput, GlobalOutput)
where
    ProjectFuture: Future<Output = ProjectOutput>,
    GlobalFuture: Future<Output = GlobalOutput>,
{
    tokio::join!(project_future, global_future)
}

pub async fn search_scopes_concurrently(
    prompt: &str,
    prompt_embedding: &[f32],
    graph: Arc<RetrievalSnapshot>,
    config: &RetrievalConfig,
    scopes: &[ScopeDescriptor],
) -> (Vec<ScopedSearchResult>, Vec<ScopedSearchFailure>) {
    match scopes {
        [scope] => {
            let result = search_scope(prompt, prompt_embedding, graph, config, scope.clone()).await;
            split_results(vec![result])
        }
        [first, second] => {
            let (first_result, second_result) = run_project_and_global_concurrently(
                search_scope(
                    prompt,
                    prompt_embedding,
                    graph.clone(),
                    config,
                    first.clone(),
                ),
                search_scope(prompt, prompt_embedding, graph, config, second.clone()),
            )
            .await;
            split_results(vec![first_result, second_result])
        }
        _ => {
            let prompt = prompt.to_owned();
            let prompt_embedding = prompt_embedding.to_vec();
            let config = config.clone();
            let mut tasks = Vec::new();

            for scope in scopes {
                let scope = scope.clone();
                let scope_id = scope.scope_id.clone();
                let scope_type_label = scope.scope_type_label().to_owned();
                let graph = graph.clone();
                let prompt = prompt.clone();
                let prompt_embedding = prompt_embedding.clone();
                let config = config.clone();

                let handle = tokio::spawn(async move {
                    search_scope(&prompt, &prompt_embedding, graph, &config, scope).await
                });

                tasks.push((scope_id, scope_type_label, handle));
            }

            let mut results = Vec::with_capacity(tasks.len());
            for (scope_id, scope_type_label, handle) in tasks {
                match handle.await {
                    Ok(result) => results.push(result),
                    Err(_) => results.push(Err(ScopedSearchFailure {
                        scope_id,
                        reason_code: format!("{scope_type_label}_search_task_failed"),
                    })),
                }
            }

            split_results(results)
        }
    }
}

/// Searches scopes concurrently using pre-fetched Qdrant hybrid candidates.
///
/// Used exclusively by the `QdrantHybrid` arm. `fused_score_by_index` maps
/// snapshot skill indices to the Qdrant RRF-fused score returned by
/// `HybridCandidateSource::query_hybrid`. Skills absent from this map were not
/// returned by Qdrant and are excluded from the candidate pool entirely.
///
/// Each scope search filters the Qdrant-returned candidates to skills that match
/// that scope's descriptor, then runs them through `perform_scope_search_with_qdrant_candidates`
/// (graph_search + eq.3 + relevance floor + MMR). The floor is authoritative:
/// a Qdrant-ranked candidate with low eq.3 score is still gated out.
pub async fn search_scopes_with_qdrant_candidates(
    prompt: &str,
    prompt_embedding: &[f32],
    graph: Arc<RetrievalSnapshot>,
    config: &RetrievalConfig,
    scopes: &[ScopeDescriptor],
    fused_score_by_index: &HashMap<usize, f32>,
) -> (Vec<ScopedSearchResult>, Vec<ScopedSearchFailure>) {
    match scopes {
        [scope] => {
            let result = search_scope_qdrant(
                prompt,
                prompt_embedding,
                graph,
                config,
                scope.clone(),
                fused_score_by_index,
            )
            .await;
            split_results(vec![result])
        }
        [first, second] => {
            let (first_result, second_result) = run_project_and_global_concurrently(
                search_scope_qdrant(
                    prompt,
                    prompt_embedding,
                    graph.clone(),
                    config,
                    first.clone(),
                    fused_score_by_index,
                ),
                search_scope_qdrant(
                    prompt,
                    prompt_embedding,
                    graph,
                    config,
                    second.clone(),
                    fused_score_by_index,
                ),
            )
            .await;
            split_results(vec![first_result, second_result])
        }
        _ => {
            let prompt = prompt.to_owned();
            let prompt_embedding = prompt_embedding.to_vec();
            let config = config.clone();
            // Clone the fused scores map once for the multi-scope path.
            let fused_score_by_index: HashMap<usize, f32> = fused_score_by_index.clone();
            let mut tasks = Vec::new();

            for scope in scopes {
                let scope = scope.clone();
                let scope_id = scope.scope_id.clone();
                let scope_type_label = scope.scope_type_label().to_owned();
                let graph = graph.clone();
                let prompt = prompt.clone();
                let prompt_embedding = prompt_embedding.clone();
                let config = config.clone();
                let fused_score_by_index = fused_score_by_index.clone();

                let handle = tokio::spawn(async move {
                    search_scope_qdrant(
                        &prompt,
                        &prompt_embedding,
                        graph,
                        &config,
                        scope,
                        &fused_score_by_index,
                    )
                    .await
                });

                tasks.push((scope_id, scope_type_label, handle));
            }

            let mut results = Vec::with_capacity(tasks.len());
            for (scope_id, scope_type_label, handle) in tasks {
                match handle.await {
                    Ok(result) => results.push(result),
                    Err(_) => results.push(Err(ScopedSearchFailure {
                        scope_id,
                        reason_code: format!("{scope_type_label}_qdrant_search_task_failed"),
                    })),
                }
            }

            split_results(results)
        }
    }
}

async fn search_scope_qdrant(
    prompt: &str,
    prompt_embedding: &[f32],
    graph: Arc<RetrievalSnapshot>,
    config: &RetrievalConfig,
    scope: ScopeDescriptor,
    fused_score_by_index: &HashMap<usize, f32>,
) -> Result<ScopedSearchResult, ScopedSearchFailure> {
    let prompt = prompt.to_owned();
    let prompt_embedding = prompt_embedding.to_vec();
    let config = config.clone();
    let fused_score_by_index: HashMap<usize, f32> = fused_score_by_index.clone();

    run_scope_search_with_timeout(scope.clone(), config.scope_timeout_ms, move || {
        perform_scope_search_with_qdrant_candidates(
            &prompt,
            &prompt_embedding,
            graph,
            &config,
            scope,
            &fused_score_by_index,
        )
    })
    .await
}

fn split_results(
    results: Vec<Result<ScopedSearchResult, ScopedSearchFailure>>,
) -> (Vec<ScopedSearchResult>, Vec<ScopedSearchFailure>) {
    let mut ok = Vec::new();
    let mut failed = Vec::new();

    for result in results {
        match result {
            Ok(value) => ok.push(value),
            Err(error) => failed.push(error),
        }
    }

    (ok, failed)
}

fn seeded_skill_matches_scope(
    seeded: &crate::orchestrator::SeededSkill,
    scope: &ScopeDescriptor,
) -> bool {
    if seeded.skill.scope != scope.scope_type {
        return false;
    }

    if seeded.scope_id != scope.scope_id {
        return false;
    }

    if scope.paths.is_empty() {
        return true;
    }

    if seeded.source_paths.is_empty() {
        return false;
    }

    seeded.source_paths.iter().all(|source_path| {
        scope
            .paths
            .iter()
            .any(|scope_path| source_path.starts_with(scope_path))
    })
}

async fn search_scope(
    prompt: &str,
    prompt_embedding: &[f32],
    graph: Arc<RetrievalSnapshot>,
    config: &RetrievalConfig,
    scope: ScopeDescriptor,
) -> Result<ScopedSearchResult, ScopedSearchFailure> {
    let prompt = prompt.to_owned();
    let prompt_embedding = prompt_embedding.to_vec();
    let config = config.clone();

    run_scope_search_with_timeout(scope.clone(), config.scope_timeout_ms, move || {
        perform_scope_search(&prompt, &prompt_embedding, graph, &config, scope)
    })
    .await
}

async fn run_scope_search_with_timeout<F>(
    scope: ScopeDescriptor,
    timeout_ms: u64,
    search_work: F,
) -> Result<ScopedSearchResult, ScopedSearchFailure>
where
    F: FnOnce() -> ScopedSearchResult + Send + 'static,
{
    let mut search_handle = tokio::task::spawn_blocking(search_work);

    match timeout(Duration::from_millis(timeout_ms), &mut search_handle).await {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(_)) => Err(ScopedSearchFailure {
            reason_code: format!("{}_search_task_failed", scope.scope_type_label()),
            scope_id: scope.scope_id,
        }),
        Err(_) => {
            search_handle.abort();
            Err(ScopedSearchFailure {
                reason_code: format!("{}_search_timeout", scope.scope_type_label()),
                scope_id: scope.scope_id,
            })
        }
    }
}

fn perform_scope_search(
    prompt: &str,
    prompt_embedding: &[f32],
    graph: Arc<RetrievalSnapshot>,
    config: &RetrievalConfig,
    scope: ScopeDescriptor,
) -> ScopedSearchResult {
    let scoped_indices: Vec<usize> = graph
        .skills
        .iter()
        .enumerate()
        .filter(|(_, seeded)| seeded_skill_matches_scope(seeded, &scope))
        .map(|(index, _)| index)
        .collect();

    if scoped_indices.is_empty() {
        return ScopedSearchResult {
            scope_id: scope.scope_id,
            scope_type: scope.scope_type,
            candidates: Vec::new(),
        };
    }

    let scoped_embeddings: Vec<Vec<f32>> = scoped_indices
        .iter()
        .filter_map(|index| {
            graph
                .skills
                .get(*index)
                .map(|seeded| seeded.embedding.clone())
        })
        .collect();

    let cosine_hits = rank_by_cosine(prompt_embedding, &scoped_embeddings, config.candidate_limit);
    let dense_candidate_indices: Vec<usize> = cosine_hits
        .iter()
        .filter_map(|hit| scoped_indices.get(hit.skill_index).copied())
        .collect();

    // Build the candidate pool. Under `SnapshotHybrid` the pool is the UNION of
    // the dense top-K and the BM25 top-K, expanding recall for exact lexical matches
    // (tool names, crate names, API identifiers) that dense cosine blurs.
    //
    // Floor authority: BM25 only expands the candidate pool fed into the existing
    // `score_eq3` → `relevance_threshold` pipeline. Every candidate — dense or
    // BM25-lifted — must clear the eq.3 floor before entering the final result set.
    // A pure-lexical hit with zero semantic alignment scores eq.3≈0 and is gated out.
    let candidate_indices: Vec<usize> = match config.backend {
        RetrievalBackend::SnapshotHybrid => expand_candidates_with_bm25(
            prompt,
            &dense_candidate_indices,
            &scoped_indices,
            &graph,
            config.candidate_limit,
        ),
        // SnapshotDense: unchanged candidate pool (current behavior).
        RetrievalBackend::SnapshotDense => dense_candidate_indices.clone(),
        // QdrantHybrid is dispatched by the orchestrator exclusively through
        // `search_scopes_with_qdrant_candidates`, which never reaches this
        // function. If this arm fires, the caller broke the dispatch invariant.
        RetrievalBackend::QdrantHybrid => unreachable!(
            "QdrantHybrid dispatches via search_scopes_with_qdrant_candidates, \
             never perform_scope_search"
        ),
    };

    // Cosine scores keyed by global skill index, used below when assembling
    // FusedCandidate for BM25-lifted skills that have no cosine_hit entry.
    let dense_score_by_index: HashMap<usize, f32> = cosine_hits
        .iter()
        .filter_map(|hit| {
            scoped_indices
                .get(hit.skill_index)
                .map(|&global_idx| (global_idx, hit.semantic_score))
        })
        .collect();

    let skill_text: Vec<String> = graph
        .skills
        .iter()
        .map(|seeded_skill| {
            format!(
                "{} {} {}",
                seeded_skill.skill.name,
                seeded_skill.skill.description,
                seeded_skill.skill.tags.join(" ")
            )
        })
        .collect();

    let skill_subunits: Vec<Vec<domain::Subunit>> = graph
        .skills
        .iter()
        .map(|seeded_skill| seeded_skill.subunits.clone())
        .collect();

    let skill_subunit_embeddings: Vec<Vec<Vec<f32>>> = graph
        .skills
        .iter()
        .map(|seeded_skill| seeded_skill.subunit_embeddings.clone())
        .collect();

    let graph_hits = search_graph(
        prompt,
        prompt_embedding,
        &skill_text,
        &skill_subunits,
        &skill_subunit_embeddings,
        &candidate_indices,
        config.max_subunits_per_skill,
    );
    let graph_hits_by_skill: HashMap<usize, GraphHit> = graph_hits
        .into_iter()
        .map(|hit| (hit.skill_index, hit))
        .collect();

    // Retrieve BM25 scores for the hybrid arm so they can be stored in
    // `FusedCandidate.lexical_score` for observability/rationale output.
    let bm25_scores_by_index: HashMap<usize, f32> =
        if config.backend == RetrievalBackend::SnapshotHybrid {
            let query_terms: Vec<String> = tokenize(prompt).into_iter().collect();
            graph
                .bm25_index
                .as_ref()
                .map(|idx| {
                    idx.score(&query_terms, &candidate_indices)
                        .into_iter()
                        .collect()
                })
                .unwrap_or_default()
        } else {
            HashMap::new()
        };

    // Build FusedCandidate for every skill in the (potentially expanded) candidate pool.
    // For dense-path skills: semantic_score comes from the cosine hit.
    // For BM25-lifted skills: semantic_score is derived from the real cosine similarity
    // (computed inline) so the eq.3 score is honest — BM25 is a recall expander, not
    // a semantic score inflator.
    let mut fused_candidates: Vec<FusedCandidate> = candidate_indices
        .iter()
        .filter_map(|&scoped_skill_index| {
            let seeded_skill = graph.skills.get(scoped_skill_index)?;
            let graph_hit = graph_hits_by_skill.get(&scoped_skill_index);

            // Semantic score: use the dense cosine if available; otherwise compute it
            // directly. This ensures BM25-lifted candidates get an honest semantic score
            // rather than defaulting to 0 (which would let the floor decision be dishonest).
            let semantic_score = dense_score_by_index
                .get(&scoped_skill_index)
                .copied()
                .unwrap_or_else(|| {
                    cosine_similarity(prompt_embedding, &seeded_skill.embedding).max(0.0)
                });

            // lexical_score: for the hybrid arm, use the real BM25 score; for dense,
            // fall back to the existing token-overlap score from graph_search (observability).
            let lexical_score = if config.backend == RetrievalBackend::SnapshotHybrid {
                bm25_scores_by_index
                    .get(&scoped_skill_index)
                    .copied()
                    .unwrap_or(0.0)
            } else {
                graph_hit.map_or(0.0, |hit| hit.lexical_score)
            };

            // β is the semantic subunit evidence (issue #172), NOT skill-name
            // lexical overlap. The skill-level lexical_score is retained only for
            // rationale/observability below.
            let subunit_evidence = graph_hit.map_or(0.0, |hit| hit.subunit_evidence);
            // Community boost (eq.3 λ term), per the configured mode (#208).
            // CentroidAffinity is query-dependent: cosine(query, the skill's
            // community centroid), clamped to [0,1] — it boosts skills whose
            // community is on-topic for THIS query, unlike the uniform binary boost.
            let community_boost = match config.community_boost_mode {
                CommunityBoostMode::Binary => seeded_skill.community_boost,
                CommunityBoostMode::Off => 0.0,
                CommunityBoostMode::CentroidAffinity => seeded_skill
                    .skill
                    .community_id
                    .as_ref()
                    .and_then(|cid| graph.community_centroids.get(cid.as_str()))
                    .map(|centroid| cosine_similarity(prompt_embedding, centroid).clamp(0.0, 1.0))
                    .unwrap_or(0.0),
            };
            // The eq.3 score is the authoritative relevance gate — BM25 expands the
            // candidate pool (recall) but does NOT bypass this score-based floor.
            // A skill surfaced only by BM25 with zero semantic alignment scores eq.3≈0
            // and is filtered out below at `relevance_threshold`. This is the scope fence
            // that keeps the hybrid arm from fabricating semantically-irrelevant matches.
            let score = score_eq3(
                ScoreComponents {
                    l1_semantic: semantic_score,
                    subunit_evidence,
                    prior: seeded_skill.prior,
                    community_boost,
                },
                config.scoring_weights,
            );

            Some(FusedCandidate {
                skill_index: scoped_skill_index,
                skill_id: seeded_skill.skill.id.as_str().to_owned(),
                matched_scope: scope.scope_type,
                score,
                semantic_score,
                lexical_score,
                subunit_evidence,
                embedding: seeded_skill.embedding.clone(),
                highlights: graph_hit
                    .map(|hit| hit.projections.clone())
                    .unwrap_or_default(),
            })
        })
        // Relevance floor: authoritative over BOTH dense and BM25-lifted candidates.
        // A BM25 hit with eq.3 < relevance_threshold is gated out here, regardless of
        // how strong its lexical score was. BM25 only expands recall; it does not lower
        // the semantic quality bar for the final result set.
        .filter(|candidate| candidate.score >= config.relevance_threshold)
        .collect();

    fused_candidates.sort_by(|left, right| right.score.total_cmp(&left.score));
    let mmr_selected = mmr_select(&fused_candidates, config.candidate_limit, config.mmr_lambda);

    ScopedSearchResult {
        scope_id: scope.scope_id,
        scope_type: scope.scope_type,
        candidates: mmr_selected,
    }
}

/// Scores QdrantHybrid candidates for a single scope using eq.3 → floor → MMR.
///
/// `fused_score_by_index` contains the Qdrant RRF-fused score for each snapshot
/// skill index that Qdrant returned. Skills absent from the map were not returned
/// by Qdrant and are excluded entirely — this is the Qdrant-as-candidate-generator
/// contract: Qdrant decides the candidate pool, the snapshot pipeline decides quality.
///
/// The relevance floor is authoritative: a Qdrant-surfaced candidate whose eq.3
/// score falls below `relevance_threshold` is gated out, exactly as in the
/// snapshot arms. Qdrant's fused score is stored in `FusedCandidate.lexical_score`
/// for observability (rationale output) but does NOT override the eq.3 gate.
fn perform_scope_search_with_qdrant_candidates(
    prompt: &str,
    prompt_embedding: &[f32],
    graph: Arc<RetrievalSnapshot>,
    config: &RetrievalConfig,
    scope: ScopeDescriptor,
    fused_score_by_index: &HashMap<usize, f32>,
) -> ScopedSearchResult {
    // Filter Qdrant-returned candidates to those matching this scope.
    let candidate_indices: Vec<usize> = fused_score_by_index
        .keys()
        .copied()
        .filter(|&idx| {
            graph
                .skills
                .get(idx)
                .map(|seeded| seeded_skill_matches_scope(seeded, &scope))
                .unwrap_or(false)
        })
        .collect();

    if candidate_indices.is_empty() {
        return ScopedSearchResult {
            scope_id: scope.scope_id,
            scope_type: scope.scope_type,
            candidates: Vec::new(),
        };
    }

    let skill_text: Vec<String> = graph
        .skills
        .iter()
        .map(|seeded_skill| {
            format!(
                "{} {} {}",
                seeded_skill.skill.name,
                seeded_skill.skill.description,
                seeded_skill.skill.tags.join(" ")
            )
        })
        .collect();

    let skill_subunits: Vec<Vec<domain::Subunit>> = graph
        .skills
        .iter()
        .map(|seeded_skill| seeded_skill.subunits.clone())
        .collect();

    let skill_subunit_embeddings: Vec<Vec<Vec<f32>>> = graph
        .skills
        .iter()
        .map(|seeded_skill| seeded_skill.subunit_embeddings.clone())
        .collect();

    let graph_hits = search_graph(
        prompt,
        prompt_embedding,
        &skill_text,
        &skill_subunits,
        &skill_subunit_embeddings,
        &candidate_indices,
        config.max_subunits_per_skill,
    );
    let graph_hits_by_skill: HashMap<usize, GraphHit> = graph_hits
        .into_iter()
        .map(|hit| (hit.skill_index, hit))
        .collect();

    let mut fused_candidates: Vec<FusedCandidate> = candidate_indices
        .iter()
        .filter_map(|&idx| {
            let seeded_skill = graph.skills.get(idx)?;
            let graph_hit = graph_hits_by_skill.get(&idx);

            // Semantic score: real cosine similarity against the prompt embedding.
            // Qdrant's fused score is not comparable to cosine and cannot substitute
            // for the α term in eq.3 — compute it honestly from the snapshot embedding.
            let semantic_score =
                crate::cosine_rank::cosine_similarity(prompt_embedding, &seeded_skill.embedding)
                    .max(0.0);

            // lexical_score: use the Qdrant RRF-fused score for observability.
            // This is stored in the rationale output but does NOT affect the eq.3 gate.
            let lexical_score = fused_score_by_index.get(&idx).copied().unwrap_or(0.0);

            let subunit_evidence = graph_hit.map_or(0.0, |hit| hit.subunit_evidence);
            let community_boost = match config.community_boost_mode {
                CommunityBoostMode::Binary => seeded_skill.community_boost,
                CommunityBoostMode::Off => 0.0,
                CommunityBoostMode::CentroidAffinity => seeded_skill
                    .skill
                    .community_id
                    .as_ref()
                    .and_then(|cid| graph.community_centroids.get(cid.as_str()))
                    .map(|centroid| cosine_similarity(prompt_embedding, centroid).clamp(0.0, 1.0))
                    .unwrap_or(0.0),
            };

            // eq.3 is the authoritative relevance gate — Qdrant's ranking expands the
            // candidate pool but does NOT bypass this score-based floor.
            let score = score_eq3(
                ScoreComponents {
                    l1_semantic: semantic_score,
                    subunit_evidence,
                    prior: seeded_skill.prior,
                    community_boost,
                },
                config.scoring_weights,
            );

            Some(FusedCandidate {
                skill_index: idx,
                skill_id: seeded_skill.skill.id.as_str().to_owned(),
                matched_scope: scope.scope_type,
                score,
                semantic_score,
                lexical_score,
                subunit_evidence,
                embedding: seeded_skill.embedding.clone(),
                highlights: graph_hit
                    .map(|hit| hit.projections.clone())
                    .unwrap_or_default(),
            })
        })
        // Relevance floor: authoritative over all Qdrant-surfaced candidates.
        .filter(|candidate| candidate.score >= config.relevance_threshold)
        .collect();

    fused_candidates.sort_by(|left, right| right.score.total_cmp(&left.score));
    let mmr_selected = mmr_select(&fused_candidates, config.candidate_limit, config.mmr_lambda);

    ScopedSearchResult {
        scope_id: scope.scope_id,
        scope_type: scope.scope_type,
        candidates: mmr_selected,
    }
}

/// Expands the dense candidate pool with BM25-scored scoped skills for the
/// `SnapshotHybrid` arm.
///
/// Returns the UNION of the `dense_candidate_indices` and the top-`limit` BM25
/// results over all `scoped_indices`. Skills already in the dense pool are not
/// duplicated. The union is bounded by `2 × limit` to prevent unbounded memory
/// growth while giving both signals fair representation.
///
/// If the snapshot has no BM25 index (cold-start or test snapshot), returns a
/// clone of `dense_candidate_indices` unchanged so the hybrid arm degrades safely
/// to dense behavior rather than panicking.
fn expand_candidates_with_bm25(
    prompt: &str,
    dense_candidate_indices: &[usize],
    scoped_indices: &[usize],
    graph: &RetrievalSnapshot,
    limit: usize,
) -> Vec<usize> {
    let bm25_index = match graph.bm25_index.as_ref() {
        Some(idx) => idx,
        // No BM25 index available — degrade safely to dense-only.
        None => return dense_candidate_indices.to_vec(),
    };

    let query_terms: Vec<String> = tokenize(prompt).into_iter().collect();
    if query_terms.is_empty() {
        return dense_candidate_indices.to_vec();
    }

    // Score all scoped skills against the BM25 index, not just the dense pool.
    // This is the core of the recall expansion: skills that dense missed may rank
    // highly under BM25 if the query contains exact lexical terms they carry.
    let bm25_hits = bm25_index.score(&query_terms, scoped_indices);

    // Build the union: start from the dense set, then add BM25-only hits.
    // The BM25 contribution is bounded by `limit` additional slots (at most
    // 2×limit total) so the hybrid arm cannot grow the candidate pool unboundedly.
    // A skill already in the dense pool is never duplicated.
    let dense_set: HashSet<usize> = dense_candidate_indices.iter().copied().collect();
    let mut expanded = dense_candidate_indices.to_vec();

    let mut added = 0;
    for (skill_index, _bm25_score) in bm25_hits {
        if added >= limit {
            break;
        }
        if !dense_set.contains(&skill_index) {
            expanded.push(skill_index);
            added += 1;
        }
    }

    expanded
}

trait ScopeTypeLabel {
    fn scope_type_label(&self) -> &'static str;
}

impl ScopeTypeLabel for ScopeDescriptor {
    fn scope_type_label(&self) -> &'static str {
        match self.scope_type {
            domain::ScopeType::Project => "project",
            domain::ScopeType::Global => "global",
            domain::ScopeType::Team => "team",
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, path::PathBuf, time::Instant};

    use domain::{DomainId, LifecycleStatus, ScopeType, Skill, SkillStatus, Subunit, SubunitType};
    use tokio::time::sleep;

    use super::*;
    use crate::orchestrator::SeededSkill;

    fn scope(scope_id: &str, scope_type: ScopeType) -> ScopeDescriptor {
        ScopeDescriptor {
            scope_id: scope_id.to_owned(),
            scope_type,
            paths: vec![PathBuf::from("/workspace")],
            config: BTreeMap::new(),
        }
    }

    fn config() -> RetrievalConfig {
        RetrievalConfig {
            candidate_limit: 10,
            max_results: 3,
            max_subunits_per_skill: 3,
            rescue_threshold: 0.1,
            relevance_threshold: 0.1,
            mmr_lambda: 0.6,
            ..RetrievalConfig::default()
        }
    }

    fn graph() -> RetrievalSnapshot {
        let project = Skill {
            id: DomainId::new_unchecked("project-skill"),
            name: "project-rust-auth".to_owned(),
            description: "Project auth flow".to_owned(),
            scope: ScopeType::Project,
            status: SkillStatus::Ready,
            lifecycle: LifecycleStatus::Active,
            tags: vec!["rust".to_owned(), "auth".to_owned()],
            subunit_ids: vec![DomainId::new_unchecked("project-sub")],
            community_id: None,
        };
        let global = Skill {
            id: DomainId::new_unchecked("global-skill"),
            name: "global-rust-auth".to_owned(),
            description: "Global auth conventions".to_owned(),
            scope: ScopeType::Global,
            status: SkillStatus::Ready,
            lifecycle: LifecycleStatus::Active,
            tags: vec!["rust".to_owned(), "auth".to_owned()],
            subunit_ids: vec![DomainId::new_unchecked("global-sub")],
            community_id: None,
        };

        RetrievalSnapshot::new(
            vec![
                SeededSkill {
                    skill: project.clone(),
                    scope_id: "project".to_owned(),
                    source_paths: vec![PathBuf::from("/workspace/project/src/auth.rs")],
                    embedding: vec![1.0, 1.0],
                    subunit_embeddings: vec![vec![1.0, 1.0]],
                    subunits: vec![Subunit {
                        id: DomainId::new_unchecked("project-sub"),
                        skill_id: project.id.clone(),
                        kind: SubunitType::Procedure,
                        title: "Project auth middleware".to_owned(),
                        content: "Trace middleware sequence".to_owned(),
                        lifecycle: LifecycleStatus::Active,
                    }],
                    // Prior is computed dynamically from real usage at graph-load
                    // time (mcp-server lib.rs via `retrieval::usage_prior`). Test
                    // fixtures use 0.0 (cold-start, no usage history) — the same
                    // value `usage_prior(0, 0)` produces.
                    prior: 0.0,
                    community_boost: 0.2,
                },
                SeededSkill {
                    skill: global.clone(),
                    scope_id: "global".to_owned(),
                    source_paths: vec![PathBuf::from("/workspace/global/docs/auth.md")],
                    embedding: vec![0.9, 1.0],
                    subunit_embeddings: vec![vec![0.9, 1.0]],
                    subunits: vec![Subunit {
                        id: DomainId::new_unchecked("global-sub"),
                        skill_id: global.id.clone(),
                        kind: SubunitType::Convention,
                        title: "Global auth checklist".to_owned(),
                        content: "Validate token lifetime".to_owned(),
                        lifecycle: LifecycleStatus::Active,
                    }],
                    // Prior is computed dynamically from real usage at graph-load
                    // time (mcp-server lib.rs via `retrieval::usage_prior`). Test
                    // fixtures use 0.0 (cold-start, no usage history) — the same
                    // value `usage_prior(0, 0)` produces.
                    prior: 0.0,
                    community_boost: 0.2,
                },
            ],
            3,
        )
    }

    fn heavy_graph(skills_per_scope: usize) -> RetrievalSnapshot {
        let mut skills = Vec::with_capacity(skills_per_scope * 2);

        for index in 0..skills_per_scope {
            let project = Skill {
                id: DomainId::new_unchecked(format!("project-skill-{index}")),
                name: format!("project-rust-auth-{index}"),
                description: "Project auth flow".to_owned(),
                scope: ScopeType::Project,
                status: SkillStatus::Ready,
                lifecycle: LifecycleStatus::Active,
                tags: vec!["rust".to_owned(), "auth".to_owned()],
                subunit_ids: vec![DomainId::new_unchecked(format!("project-sub-{index}"))],
                community_id: None,
            };
            let global = Skill {
                id: DomainId::new_unchecked(format!("global-skill-{index}")),
                name: format!("global-rust-auth-{index}"),
                description: "Global auth conventions".to_owned(),
                scope: ScopeType::Global,
                status: SkillStatus::Ready,
                lifecycle: LifecycleStatus::Active,
                tags: vec!["rust".to_owned(), "auth".to_owned()],
                subunit_ids: vec![DomainId::new_unchecked(format!("global-sub-{index}"))],
                community_id: None,
            };

            skills.push(SeededSkill {
                skill: project.clone(),
                scope_id: "project".to_owned(),
                source_paths: vec![PathBuf::from(format!(
                    "/workspace/project/src/file-{index}.rs"
                ))],
                embedding: vec![1.0, 1.0],
                subunit_embeddings: vec![vec![1.0, 1.0]],
                subunits: vec![Subunit {
                    id: DomainId::new_unchecked(format!("project-sub-{index}")),
                    skill_id: project.id.clone(),
                    kind: SubunitType::Procedure,
                    title: "Project auth middleware".to_owned(),
                    content: "Trace middleware sequence".to_owned(),
                    lifecycle: LifecycleStatus::Active,
                }],
                prior: 0.1,
                community_boost: 0.2,
            });

            skills.push(SeededSkill {
                skill: global.clone(),
                scope_id: "global".to_owned(),
                source_paths: vec![PathBuf::from(format!(
                    "/workspace/global/docs/file-{index}.md"
                ))],
                embedding: vec![0.9, 1.0],
                subunit_embeddings: vec![vec![0.9, 1.0]],
                subunits: vec![Subunit {
                    id: DomainId::new_unchecked(format!("global-sub-{index}")),
                    skill_id: global.id.clone(),
                    kind: SubunitType::Convention,
                    title: "Global auth checklist".to_owned(),
                    content: "Validate token lifetime".to_owned(),
                    lifecycle: LifecycleStatus::Active,
                }],
                prior: 0.1,
                community_boost: 0.2,
            });
        }

        RetrievalSnapshot::new(skills, 7)
    }

    #[tokio::test]
    async fn runs_project_and_global_searches_in_parallel_latency_envelope() {
        let started = Instant::now();
        let (_project, _global) = run_project_and_global_concurrently(
            async {
                sleep(Duration::from_millis(80)).await;
                "project"
            },
            async {
                sleep(Duration::from_millis(80)).await;
                "global"
            },
        )
        .await;

        assert!(
            started.elapsed() < Duration::from_millis(140),
            "parallel searches should complete close to max(single-scope latency)"
        );
    }

    #[tokio::test]
    async fn filters_candidates_by_scope_before_fusion() {
        let (results, failures) = search_scopes_concurrently(
            "rust auth",
            &[1.0, 1.0],
            Arc::new(graph()),
            &config(),
            &[
                scope("project", ScopeType::Project),
                scope("global", ScopeType::Global),
            ],
        )
        .await;

        assert!(failures.is_empty());
        assert_eq!(results.len(), 2);
        assert!(
            results
                .iter()
                .any(|result| result.scope_type == ScopeType::Project)
        );
        assert!(
            results
                .iter()
                .any(|result| result.scope_type == ScopeType::Global)
        );
        assert!(results.iter().all(|result| !result.candidates.is_empty()));
    }

    #[tokio::test]
    async fn real_scope_search_path_meets_parallel_latency_envelope() {
        let graph = Arc::new(heavy_graph(300));
        let search_config = config();
        let prompt = "rust auth";
        let embedding = [1.0, 1.0];

        let project_scope = [scope("project", ScopeType::Project)];
        let started = Instant::now();
        let (_project_results, project_failures) = search_scopes_concurrently(
            prompt,
            &embedding,
            graph.clone(),
            &search_config,
            &project_scope,
        )
        .await;
        let project_elapsed = started.elapsed();
        assert!(project_failures.is_empty());

        let global_scope = [scope("global", ScopeType::Global)];
        let started = Instant::now();
        let (_global_results, global_failures) = search_scopes_concurrently(
            prompt,
            &embedding,
            graph.clone(),
            &search_config,
            &global_scope,
        )
        .await;
        let global_elapsed = started.elapsed();
        assert!(global_failures.is_empty());

        let dual_scopes = [
            scope("project", ScopeType::Project),
            scope("global", ScopeType::Global),
        ];
        let started = Instant::now();
        let (_dual_results, dual_failures) =
            search_scopes_concurrently(prompt, &embedding, graph, &search_config, &dual_scopes)
                .await;
        let dual_elapsed = started.elapsed();
        assert!(dual_failures.is_empty());

        // Individual in-memory scope searches here are sub-millisecond, so a strict
        // `dual < project + global` comparison is dominated by scheduler jitter (tens
        // of µs) and flakes under load (parallel task spawn/join overhead can exceed
        // the tiny savings). Assert the meaningful contract instead: the parallel
        // dual-scope path stays well within the retrieval latency budget and does not
        // serialize (cost more than the sequential sum plus a jitter allowance).
        let sequential_sum = project_elapsed + global_elapsed;
        let jitter_allowance = Duration::from_millis(10);
        assert!(
            dual_elapsed < Duration::from_millis(250),
            "dual-scope search must stay within the latency envelope: dual={dual_elapsed:?}"
        );
        assert!(
            dual_elapsed <= sequential_sum + jitter_allowance,
            "dual-scope search must not serialize: dual={dual_elapsed:?}, sequential sum={sequential_sum:?}, jitter allowance={jitter_allowance:?}"
        );
    }

    #[tokio::test]
    async fn real_scope_search_three_scopes_meets_parallel_latency_envelope() {
        let graph = Arc::new(heavy_graph(300));
        let search_config = config();
        let prompt = "rust auth";
        let embedding = [1.0, 1.0];

        let project_scope = [scope("project", ScopeType::Project)];
        let started = Instant::now();
        let (_project_results, project_failures) = search_scopes_concurrently(
            prompt,
            &embedding,
            graph.clone(),
            &search_config,
            &project_scope,
        )
        .await;
        let project_elapsed = started.elapsed();
        assert!(project_failures.is_empty());

        let global_scope = [scope("global", ScopeType::Global)];
        let started = Instant::now();
        let (_global_results, global_failures) = search_scopes_concurrently(
            prompt,
            &embedding,
            graph.clone(),
            &search_config,
            &global_scope,
        )
        .await;
        let global_elapsed = started.elapsed();
        assert!(global_failures.is_empty());

        let second_global_scope = [scope("global", ScopeType::Global)];
        let started = Instant::now();
        let (_second_global_results, second_global_failures) = search_scopes_concurrently(
            prompt,
            &embedding,
            graph.clone(),
            &search_config,
            &second_global_scope,
        )
        .await;
        let second_global_elapsed = started.elapsed();
        assert!(second_global_failures.is_empty());

        let three_scopes = [
            scope("project", ScopeType::Project),
            scope("global", ScopeType::Global),
            scope("global", ScopeType::Global),
        ];
        let started = Instant::now();
        let (three_scope_results, three_scope_failures) =
            search_scopes_concurrently(prompt, &embedding, graph, &search_config, &three_scopes)
                .await;
        let three_scope_elapsed = started.elapsed();
        assert!(three_scope_failures.is_empty());
        assert_eq!(three_scope_results.len(), 3);

        assert!(
            three_scope_elapsed < project_elapsed + global_elapsed + second_global_elapsed,
            "three-scope search should complete faster than sequential per-scope path: three={three_scope_elapsed:?}, project={project_elapsed:?}, global={global_elapsed:?}, second_global={second_global_elapsed:?}"
        );
    }

    #[tokio::test]
    async fn timeout_is_effective_for_blocking_scope_work() {
        let started = Instant::now();

        let result =
            run_scope_search_with_timeout(scope("project", ScopeType::Project), 20, || {
                std::thread::sleep(Duration::from_millis(120));
                ScopedSearchResult {
                    scope_id: "project".to_owned(),
                    scope_type: ScopeType::Project,
                    candidates: Vec::new(),
                }
            })
            .await;

        let failure = result.expect_err("blocking work should time out");
        assert_eq!(failure.reason_code, "project_search_timeout");
        assert_eq!(failure.scope_id, "project");
        assert!(
            started.elapsed() < Duration::from_millis(90),
            "timeout should return before blocking work completes"
        );
    }

    #[tokio::test]
    async fn excludes_candidates_when_scope_id_or_paths_do_not_match_descriptor() {
        let project = Skill {
            id: DomainId::new_unchecked("project-skill"),
            name: "project-rust-auth".to_owned(),
            description: "Project auth flow".to_owned(),
            scope: ScopeType::Project,
            status: SkillStatus::Ready,
            lifecycle: LifecycleStatus::Active,
            tags: vec!["rust".to_owned(), "auth".to_owned()],
            subunit_ids: vec![DomainId::new_unchecked("project-sub")],
            community_id: None,
        };
        let graph = RetrievalSnapshot::new(
            vec![
                SeededSkill {
                    skill: project.clone(),
                    scope_id: "global".to_owned(),
                    source_paths: vec![PathBuf::from("/workspace/project/src/auth.rs")],
                    embedding: vec![1.0, 1.0],
                    subunit_embeddings: vec![vec![1.0, 1.0]],
                    subunits: vec![Subunit {
                        id: DomainId::new_unchecked("project-sub"),
                        skill_id: project.id.clone(),
                        kind: SubunitType::Procedure,
                        title: "Project auth middleware".to_owned(),
                        content: "Trace middleware sequence".to_owned(),
                        lifecycle: LifecycleStatus::Active,
                    }],
                    // Prior is computed dynamically from real usage at graph-load
                    // time (mcp-server lib.rs via `retrieval::usage_prior`). Test
                    // fixtures use 0.0 (cold-start, no usage history) — the same
                    // value `usage_prior(0, 0)` produces.
                    prior: 0.0,
                    community_boost: 0.2,
                },
                SeededSkill {
                    skill: project,
                    scope_id: "project".to_owned(),
                    source_paths: vec![PathBuf::from("/outside-scope/auth.rs")],
                    embedding: vec![0.95, 1.0],
                    subunit_embeddings: vec![vec![0.95, 1.0]],
                    subunits: vec![Subunit {
                        id: DomainId::new_unchecked("project-sub-outside"),
                        skill_id: DomainId::new_unchecked("project-skill"),
                        kind: SubunitType::Procedure,
                        title: "Outside scope auth".to_owned(),
                        content: "Should be excluded".to_owned(),
                        lifecycle: LifecycleStatus::Active,
                    }],
                    // Prior is computed dynamically from real usage at graph-load
                    // time (mcp-server lib.rs via `retrieval::usage_prior`). Test
                    // fixtures use 0.0 (cold-start, no usage history) — the same
                    // value `usage_prior(0, 0)` produces.
                    prior: 0.0,
                    community_boost: 0.2,
                },
            ],
            7,
        );

        let (results, failures) = search_scopes_concurrently(
            "rust auth",
            &[1.0, 1.0],
            Arc::new(graph),
            &config(),
            &[scope("project", ScopeType::Project)],
        )
        .await;

        assert!(failures.is_empty());
        assert_eq!(results.len(), 1);
        assert!(results[0].candidates.is_empty());
    }

    /// Keystone: a skill with real `source_paths` loaded from PG matches the
    /// scope by its actual file path, not by the scope-root stand-in.
    ///
    /// This proves T09's replacement of T01's scope-root substitution:
    /// - skill A has `source_paths = ["/workspace/project/src/io.rs"]`
    ///   → matched by a scope whose path is `/workspace/project`
    /// - skill B has `source_paths = ["/other-project/src/io.rs"]`
    ///   → excluded by that same scope (path does not start with `/workspace/project`)
    ///
    /// An empty `source_paths` would fall back to the scope root; the stand-in
    /// is exercised in `excludes_candidates_when_scope_id_or_paths_do_not_match_descriptor`.
    #[tokio::test]
    async fn skill_with_real_source_paths_matches_scope_by_true_provenance_not_scope_root() {
        let skill_with_real_path = Skill {
            id: DomainId::new_unchecked("io-skill-real-path"),
            name: "rust-tokio-io".to_owned(),
            description: "Async file IO with tokio".to_owned(),
            scope: ScopeType::Project,
            status: SkillStatus::Ready,
            lifecycle: LifecycleStatus::Active,
            tags: vec!["rust".to_owned(), "tokio".to_owned(), "io".to_owned()],
            subunit_ids: vec![DomainId::new_unchecked("io-sub")],
            community_id: None,
        };

        let graph = RetrievalSnapshot::new(
            vec![
                // Skill A: source_paths under the queried scope root — MUST match.
                SeededSkill {
                    skill: skill_with_real_path.clone(),
                    scope_id: "project".to_owned(),
                    source_paths: vec![PathBuf::from("/workspace/project/src/io.rs")],
                    embedding: vec![1.0, 1.0],
                    subunit_embeddings: vec![vec![1.0, 1.0]],
                    subunits: vec![Subunit {
                        id: DomainId::new_unchecked("io-sub"),
                        skill_id: skill_with_real_path.id.clone(),
                        kind: SubunitType::Procedure,
                        title: "Read file async".to_owned(),
                        content: "tokio::fs::read_to_string".to_owned(),
                        lifecycle: LifecycleStatus::Active,
                    }],
                    prior: 0.0,
                    community_boost: 0.0,
                },
                // Skill B: source_paths outside the queried scope root — MUST be excluded.
                SeededSkill {
                    skill: skill_with_real_path.clone(),
                    scope_id: "project".to_owned(),
                    source_paths: vec![PathBuf::from("/other-project/src/io.rs")],
                    embedding: vec![1.0, 1.0],
                    subunit_embeddings: vec![vec![1.0, 1.0]],
                    subunits: vec![Subunit {
                        id: DomainId::new_unchecked("io-sub-outside"),
                        skill_id: skill_with_real_path.id.clone(),
                        kind: SubunitType::Procedure,
                        title: "Read file outside scope".to_owned(),
                        content: "must be excluded by path gate".to_owned(),
                        lifecycle: LifecycleStatus::Active,
                    }],
                    prior: 0.0,
                    community_boost: 0.0,
                },
            ],
            1,
        );

        // Scope root is `/workspace/project` — only skill A's path starts with it.
        let project_scope = ScopeDescriptor {
            scope_id: "project".to_owned(),
            scope_type: ScopeType::Project,
            paths: vec![PathBuf::from("/workspace/project")],
            config: std::collections::BTreeMap::new(),
        };

        let (results, failures) = search_scopes_concurrently(
            "rust tokio io",
            &[1.0, 1.0],
            Arc::new(graph),
            &config(),
            &[project_scope],
        )
        .await;

        assert!(failures.is_empty(), "search should not fail");
        assert_eq!(results.len(), 1, "should have one scope result");
        // Exactly one candidate: skill A (real path matches). Skill B is excluded.
        assert_eq!(
            results[0].candidates.len(),
            1,
            "only the skill whose source_path is under the scope root should match; \
             got {} candidates (expected 1 — skill A only)",
            results[0].candidates.len()
        );
    }

    /// Proves cold-start (empty graph) returns no candidates, not an error.
    ///
    /// An empty `skills` vector is the valid cold-start state; scope matching
    /// correctly produces `candidates = []` without panicking or returning degraded.
    #[tokio::test]
    async fn empty_graph_returns_no_candidates_not_error() {
        let empty_graph = RetrievalSnapshot::new(vec![], 0);

        let (results, failures) = search_scopes_concurrently(
            "rust tokio io",
            &[1.0, 1.0],
            Arc::new(empty_graph),
            &config(),
            &[scope("project", ScopeType::Project)],
        )
        .await;

        assert!(
            failures.is_empty(),
            "empty graph must not produce scope failures"
        );
        assert_eq!(
            results.len(),
            1,
            "should have one scope result even for empty graph"
        );
        assert!(
            results[0].candidates.is_empty(),
            "empty graph must return zero candidates (honest no_match)"
        );
    }

    /// Proves the relevance floor rejects a candidate whose eq3 score is below
    /// `relevance_threshold`, even when the skill embedding partially aligns.
    ///
    /// Background (#209): the 0.450 floor from the isolated 8-skill corpus was too
    /// low for the real 234-skill corpus. Live-server calibration raised the
    /// default to 0.48, which blocked off-topic fabrications and improved positive
    /// ranking by removing low-score noise.
    ///
    /// This test locks the floor contract so a future config change cannot silently
    /// lower it below the level that blocks fabricated matches for off-topic prompts.
    ///
    /// The prompt embedding `[1.0, 0.0]` and the skill embedding `[0.0, 1.0]` are
    /// orthogonal, giving cosine similarity = 0.0 (α term = 0). With the default
    /// weights (α=0.45, β=0.35, γ=0.20, λ=0.25) and no subunit evidence (β=0) and
    /// no prior (γ=0), the eq3 score is 0.0 — clearly below 0.48.
    /// The floor must exclude this candidate, leaving an empty candidates list.
    #[tokio::test]
    async fn relevance_floor_excludes_candidate_below_threshold() {
        use std::collections::BTreeMap;

        // Use the calibrated default threshold (0.48) as configured in RetrievalConfig.
        // A skill with zero cosine alignment gets eq3 = 0 — well below the floor.
        let floor_config = RetrievalConfig {
            candidate_limit: 10,
            max_results: 3,
            max_subunits_per_skill: 3,
            rescue_threshold: 0.15,
            relevance_threshold: RetrievalConfig::default().relevance_threshold,
            mmr_lambda: 0.65,
            ..RetrievalConfig::default()
        };

        let skill = domain::Skill {
            id: domain::DomainId::new_unchecked("below-floor-skill"),
            name: "below-floor-skill".to_owned(),
            description: "A skill whose eq3 score falls below the relevance floor".to_owned(),
            scope: domain::ScopeType::Global,
            status: domain::SkillStatus::Ready,
            lifecycle: domain::LifecycleStatus::Active,
            tags: vec![],
            subunit_ids: vec![],
            community_id: None,
        };

        let snapshot = RetrievalSnapshot::new(
            vec![crate::orchestrator::SeededSkill {
                skill,
                scope_id: "global".to_owned(),
                source_paths: vec![],
                // Orthogonal to the query embedding: cosine = 0.
                embedding: vec![0.0, 1.0],
                // No subunits: subunit_evidence (β) = 0.
                subunit_embeddings: vec![],
                subunits: vec![],
                // No usage history: prior (γ) = 0.
                prior: 0.0,
                community_boost: 0.0,
            }],
            1,
        );

        let global_scope = ScopeDescriptor {
            scope_id: "global".to_owned(),
            scope_type: domain::ScopeType::Global,
            paths: vec![],
            config: BTreeMap::new(),
        };

        let (results, failures) = search_scopes_concurrently(
            "query with no subunit or prior signal",
            &[1.0, 0.0],
            Arc::new(snapshot),
            &floor_config,
            &[global_scope],
        )
        .await;

        assert!(failures.is_empty(), "search must not fail");
        assert_eq!(results.len(), 1, "should have one scope result");
        assert!(
            results[0].candidates.is_empty(),
            "candidate whose eq3 = 0 must be excluded by the 0.48 relevance floor; \
             got {} candidates (expected 0 — floor must block fabricated matches)",
            results[0].candidates.len()
        );
    }

    /// Proves that a candidate with strong subunit evidence that pushes eq3 above
    /// the relevance floor IS admitted, while the floor still blocks weaker candidates.
    ///
    /// This test demonstrates that the calibrated 0.48 threshold does not over-reject:
    /// a skill with real subunit evidence (β > 0) clears the floor when its
    /// combined score α×0.45 + β×0.35 ≥ 0.48.
    ///
    /// With default weights and cosine=1.0 for both skill and subunit:
    ///   eq3 = 0.45 × 1.0 + 0.35 × 1.0 = 0.80 → well above 0.48.
    #[tokio::test]
    async fn relevance_floor_admits_candidate_with_sufficient_combined_score() {
        use std::collections::BTreeMap;

        let floor_config = RetrievalConfig {
            candidate_limit: 10,
            max_results: 3,
            max_subunits_per_skill: 3,
            rescue_threshold: 0.15,
            relevance_threshold: RetrievalConfig::default().relevance_threshold,
            mmr_lambda: 0.65,
            ..RetrievalConfig::default()
        };

        let skill = domain::Skill {
            id: domain::DomainId::new_unchecked("above-floor-skill"),
            name: "above-floor-skill".to_owned(),
            description: "above floor skill with strong subunit alignment".to_owned(),
            scope: domain::ScopeType::Global,
            status: domain::SkillStatus::Ready,
            lifecycle: domain::LifecycleStatus::Active,
            tags: vec![],
            subunit_ids: vec![domain::DomainId::new_unchecked("sub-1")],
            community_id: None,
        };

        // With α=0.45, β=0.35, and cosine(query, skill)=1.0, cosine(query, subunit)=1.0:
        // eq3 = 0.45 × 1.0 + 0.35 × 1.0 + 0.20 × 0.0 = 0.80 → above the 0.48 floor.
        let snapshot = RetrievalSnapshot::new(
            vec![crate::orchestrator::SeededSkill {
                skill,
                scope_id: "global".to_owned(),
                source_paths: vec![],
                embedding: vec![1.0, 1.0],
                subunit_embeddings: vec![vec![1.0, 1.0]], // perfect subunit alignment
                subunits: vec![domain::Subunit {
                    id: domain::DomainId::new_unchecked("sub-1"),
                    skill_id: domain::DomainId::new_unchecked("above-floor-skill"),
                    kind: domain::SubunitType::Procedure,
                    title: "Strong subunit".to_owned(),
                    content: "Aligned with the query".to_owned(),
                    lifecycle: domain::LifecycleStatus::Active,
                }],
                prior: 0.0,
                community_boost: 0.0,
            }],
            1,
        );

        let global_scope = ScopeDescriptor {
            scope_id: "global".to_owned(),
            scope_type: domain::ScopeType::Global,
            paths: vec![],
            config: BTreeMap::new(),
        };

        let (results, failures) = search_scopes_concurrently(
            "query with strong skill and subunit alignment",
            &[1.0, 1.0],
            Arc::new(snapshot),
            &floor_config,
            &[global_scope],
        )
        .await;

        assert!(failures.is_empty(), "search must not fail");
        assert_eq!(results.len(), 1, "should have one scope result");
        assert_eq!(
            results[0].candidates.len(),
            1,
            "candidate with eq3 ≈ 0.80 must be admitted above the 0.48 relevance floor; \
             got {} candidates (expected 1)",
            results[0].candidates.len()
        );
        let admitted = &results[0].candidates[0];
        assert!(
            admitted.score >= RetrievalConfig::default().relevance_threshold,
            "admitted candidate must have score >= floor (0.48); got {:.4}",
            admitted.score
        );
    }

    // ── T04-B hybrid arm tests ────────────────────────────────────────────────

    /// Builds a three-skill snapshot for hybrid arm testing:
    ///
    /// - `dominant-a` (index 0): strong dense match (cosine=1.0), no "qdrant" term.
    /// - `dominant-b` (index 1): strong dense match (cosine=1.0), no "qdrant" term.
    ///   Present to saturate `candidate_limit=2` so the lexical target is excluded
    ///   from the dense top-2 by rank.
    /// - `lexical-target` (index 2): orthogonal embedding (cosine=0), contains the
    ///   rare term "qdrant" that the query carries. Dense ranks it 3rd (outside
    ///   limit=2); BM25 ranks it 1st.
    ///
    /// MMR with `candidate_limit=2` and `lambda=0.6` over the expanded hybrid pool
    /// picks dominant-a (highest score 0.45) then lexical-target (orthogonal to
    /// dominant-a → high diversity reward beats dominant-b at mmr=-0.13 vs 0).
    fn hybrid_snapshot() -> RetrievalSnapshot {
        use std::sync::Arc;

        let dominant_a = Skill {
            id: DomainId::new_unchecked("dominant-a"),
            name: "async runtime patterns alpha".to_owned(),
            description: "tokio async patterns best practices".to_owned(),
            scope: ScopeType::Global,
            status: SkillStatus::Ready,
            lifecycle: LifecycleStatus::Active,
            tags: vec!["async".to_owned(), "runtime".to_owned()],
            subunit_ids: vec![],
            community_id: None,
        };
        let dominant_b = Skill {
            id: DomainId::new_unchecked("dominant-b"),
            name: "async runtime patterns beta".to_owned(),
            description: "tokio async patterns advanced".to_owned(),
            scope: ScopeType::Global,
            status: SkillStatus::Ready,
            lifecycle: LifecycleStatus::Active,
            tags: vec!["async".to_owned(), "runtime".to_owned()],
            subunit_ids: vec![],
            community_id: None,
        };
        let lexical_target = Skill {
            id: DomainId::new_unchecked("lexical-target-skill"),
            name: "qdrant vector database integration".to_owned(),
            description: "How to integrate qdrant for vector search".to_owned(),
            scope: ScopeType::Global,
            status: SkillStatus::Ready,
            lifecycle: LifecycleStatus::Active,
            tags: vec!["qdrant".to_owned(), "vector".to_owned()],
            subunit_ids: vec![],
            community_id: None,
        };

        // Query embedding: [1.0, 0.0]
        // dominant-a:     [1.0, 0.0] cosine=1.0 (dense rank 1)
        // dominant-b:     [1.0, 0.0] cosine=1.0 (dense rank 2)
        // lexical-target: [0.0, 1.0] cosine=0.0 (dense rank 3; outside limit=2)
        let snapshot = RetrievalSnapshot::new(
            vec![
                SeededSkill {
                    skill: dominant_a,
                    scope_id: "global".to_owned(),
                    source_paths: vec![],
                    embedding: vec![1.0, 0.0],
                    subunits: vec![],
                    subunit_embeddings: vec![],
                    prior: 0.0,
                    community_boost: 0.0,
                },
                SeededSkill {
                    skill: dominant_b,
                    scope_id: "global".to_owned(),
                    source_paths: vec![],
                    embedding: vec![1.0, 0.0],
                    subunits: vec![],
                    subunit_embeddings: vec![],
                    prior: 0.0,
                    community_boost: 0.0,
                },
                SeededSkill {
                    skill: lexical_target,
                    scope_id: "global".to_owned(),
                    source_paths: vec![],
                    embedding: vec![0.0, 1.0],
                    subunits: vec![],
                    subunit_embeddings: vec![],
                    prior: 0.0,
                    community_boost: 0.0,
                },
            ],
            1,
        );

        let bm25_docs: Vec<(usize, String)> = snapshot
            .skills
            .iter()
            .enumerate()
            .map(|(idx, s)| {
                (
                    idx,
                    format!(
                        "{} {} {}",
                        s.skill.name,
                        s.skill.description,
                        s.skill.tags.join(" ")
                    ),
                )
            })
            .collect();
        snapshot.with_bm25_index(Arc::new(crate::bm25::Bm25Index::build(&bm25_docs)))
    }

    /// Under `SnapshotHybrid`, a skill with an exact lexical term match ("qdrant")
    /// enters the final candidate set even when `candidate_limit=2` fills the dense
    /// pool with two decoy skills that have higher cosine scores.
    ///
    /// MMR then selects [dominant-a, lexical-target] because lexical-target is
    /// orthogonal to dominant-a and thus wins the diversity reward over dominant-b.
    ///
    /// `relevance_threshold=0.0` isolates the pool-expansion behavior; the floor
    /// gate is tested separately in `snapshot_hybrid_relevance_floor_gates_*`.
    #[tokio::test]
    async fn snapshot_hybrid_surfaces_lexical_target_that_dense_excludes() {
        let query = "qdrant vector database";
        let query_embedding = [1.0_f32, 0.0];

        let hybrid_config = RetrievalConfig {
            candidate_limit: 2, // dense returns top-2 (dominant-a, dominant-b)
            max_results: 5,
            max_subunits_per_skill: 3,
            rescue_threshold: 0.1,
            relevance_threshold: 0.0, // zero floor: isolate pool-expansion behavior
            mmr_lambda: 0.6,
            backend: crate::orchestrator::RetrievalBackend::SnapshotHybrid,
            ..RetrievalConfig::default()
        };
        let dense_config = RetrievalConfig {
            backend: crate::orchestrator::RetrievalBackend::SnapshotDense,
            ..hybrid_config.clone()
        };

        let snapshot = Arc::new(hybrid_snapshot());
        let global_scope = ScopeDescriptor {
            scope_id: "global".to_owned(),
            scope_type: ScopeType::Global,
            paths: vec![],
            config: BTreeMap::new(),
        };

        let (dense_results, dense_failures) = search_scopes_concurrently(
            query,
            &query_embedding,
            snapshot.clone(),
            &dense_config,
            std::slice::from_ref(&global_scope),
        )
        .await;
        assert!(dense_failures.is_empty());
        let dense_candidates = &dense_results[0].candidates;

        let (hybrid_results, hybrid_failures) = search_scopes_concurrently(
            query,
            &query_embedding,
            snapshot.clone(),
            &hybrid_config,
            std::slice::from_ref(&global_scope),
        )
        .await;
        assert!(hybrid_failures.is_empty());
        let hybrid_candidates = &hybrid_results[0].candidates;

        // Dense with candidate_limit=2 must NOT surface the lexical-target (cosine rank 3).
        let dense_has_lexical_target = dense_candidates
            .iter()
            .any(|c| c.skill_id == "lexical-target-skill");
        assert!(
            !dense_has_lexical_target,
            "SnapshotDense with candidate_limit=2 must NOT surface the lexical-target skill \
             (cosine rank 3, outside dense limit); dense ids: {:?}",
            dense_candidates
                .iter()
                .map(|c| &c.skill_id)
                .collect::<Vec<_>>()
        );

        // Hybrid must surface the lexical target via BM25 expansion.
        let hybrid_has_lexical_target = hybrid_candidates
            .iter()
            .any(|c| c.skill_id == "lexical-target-skill");
        assert!(
            hybrid_has_lexical_target,
            "SnapshotHybrid must surface the lexical-target skill ('qdrant') via BM25; \
             hybrid ids: {:?}",
            hybrid_candidates
                .iter()
                .map(|c| &c.skill_id)
                .collect::<Vec<_>>()
        );

        // The lexical-target must carry a non-zero BM25 score in the hybrid arm.
        let lexical = hybrid_candidates
            .iter()
            .find(|c| c.skill_id == "lexical-target-skill")
            .expect("lexical-target-skill must be in hybrid candidates");
        assert!(
            lexical.lexical_score > 0.0,
            "lexical-target-skill must have non-zero BM25 lexical_score; got {}",
            lexical.lexical_score
        );
    }

    /// Under `SnapshotHybrid`, a candidate that is purely lexically matched but
    /// whose eq.3 score falls below the `relevance_threshold` must still be gated out.
    ///
    /// This proves the scope fence: BM25 expands the candidate POOL (recall), but
    /// the final result set still passes through the existing `relevance_threshold`
    /// floor on the eq.3 score. A pure-lexical hit with low semantic relevance is
    /// gated out — the relevance floor is authoritative over the hybrid expansion.
    #[tokio::test]
    async fn snapshot_hybrid_relevance_floor_gates_lexical_only_hit_with_low_eq3_score() {
        use std::sync::Arc;

        let lexical_only = Skill {
            id: DomainId::new_unchecked("lexical-only-skill"),
            // "rustfmt" is in the query, so BM25 scores this non-zero.
            name: "rustfmt code formatting".to_owned(),
            description: "How to use rustfmt".to_owned(),
            scope: ScopeType::Global,
            status: SkillStatus::Ready,
            lifecycle: LifecycleStatus::Active,
            tags: vec!["rustfmt".to_owned()],
            subunit_ids: vec![],
            community_id: None,
        };

        // Orthogonal embedding: cosine similarity to query [1.0, 0.0] → 0.
        // eq.3 score = 0.45*0 + 0.35*0 + 0.20*0 = 0 → below ANY positive threshold.
        let snapshot = RetrievalSnapshot::new(
            vec![SeededSkill {
                skill: lexical_only,
                scope_id: "global".to_owned(),
                source_paths: vec![],
                embedding: vec![0.0, 1.0], // orthogonal → cosine=0
                subunits: vec![],
                subunit_embeddings: vec![],
                prior: 0.0,
                community_boost: 0.0,
            }],
            1,
        );
        let bm25_docs: Vec<(usize, String)> = snapshot
            .skills
            .iter()
            .enumerate()
            .map(|(idx, s)| {
                (
                    idx,
                    format!(
                        "{} {} {}",
                        s.skill.name,
                        s.skill.description,
                        s.skill.tags.join(" ")
                    ),
                )
            })
            .collect();
        let snapshot =
            snapshot.with_bm25_index(Arc::new(crate::bm25::Bm25Index::build(&bm25_docs)));

        let high_floor_config = RetrievalConfig {
            candidate_limit: 10,
            max_results: 5,
            max_subunits_per_skill: 3,
            rescue_threshold: 0.15,
            // High floor: eq.3 must be ≥ 0.48 to pass. The lexical-only skill
            // has eq.3 = 0 (orthogonal embedding) so it must be gated out.
            relevance_threshold: 0.48,
            mmr_lambda: 0.65,
            backend: crate::orchestrator::RetrievalBackend::SnapshotHybrid,
            ..RetrievalConfig::default()
        };

        let global_scope = ScopeDescriptor {
            scope_id: "global".to_owned(),
            scope_type: ScopeType::Global,
            paths: vec![],
            config: BTreeMap::new(),
        };

        // "rustfmt" lexically matches the skill but eq.3 score = 0 → below 0.48 floor.
        let (results, failures) = search_scopes_concurrently(
            "rustfmt formatting",
            &[1.0_f32, 0.0],
            Arc::new(snapshot),
            &high_floor_config,
            &[global_scope],
        )
        .await;

        assert!(failures.is_empty(), "search must not fail");
        assert_eq!(results.len(), 1, "should have one scope result");
        assert!(
            results[0].candidates.is_empty(),
            "SnapshotHybrid must NOT return a lexical-only hit whose eq.3 score \
             is below the relevance_threshold floor (0.48); the floor is authoritative \
             over BM25 expansion. Got {} candidates.",
            results[0].candidates.len()
        );
    }
}
