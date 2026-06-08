use std::{collections::HashMap, future::Future, sync::Arc, time::Duration};

use domain::ScopeDescriptor;
use tokio::time::timeout;

use crate::{
    cosine_rank::{cosine_similarity, rank_by_cosine},
    fusion::{FusedCandidate, mmr_select},
    graph_search::{GraphHit, search_graph},
    orchestrator::{CommunityBoostMode, RetrievalConfig, RetrievalSnapshot},
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
    let candidate_indices: Vec<usize> = cosine_hits
        .iter()
        .filter_map(|hit| scoped_indices.get(hit.skill_index).copied())
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

    let mut fused_candidates: Vec<FusedCandidate> = cosine_hits
        .iter()
        .filter_map(|cosine_hit| {
            let scoped_skill_index = *scoped_indices.get(cosine_hit.skill_index)?;
            let seeded_skill = graph.skills.get(scoped_skill_index)?;
            let graph_hit = graph_hits_by_skill.get(&scoped_skill_index);
            let lexical_score = graph_hit.map_or(0.0, |hit| hit.lexical_score);
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
            let score = score_eq3(
                ScoreComponents {
                    l1_semantic: cosine_hit.semantic_score,
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
                semantic_score: cosine_hit.semantic_score,
                lexical_score,
                subunit_evidence,
                embedding: seeded_skill.embedding.clone(),
                highlights: graph_hit
                    .map(|hit| hit.projections.clone())
                    .unwrap_or_default(),
            })
        })
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
    /// Background (#192): the old default floor (0.20) was too low for
    /// `nomic-embed-text` — its cosine similarities are inflated for all text
    /// pairs, including off-topic ones. Live calibration with per-query-tagged
    /// scoring on the isolated 8-skill quality corpus (2026-06-07, 772 events)
    /// established a threshold of 0.450 sitting in the 0.0179-wide gap between
    /// the worst negative's eq3 (0.4386, kubernetes TLS) and the lowest
    /// true-positive disjoint hit's eq3 (0.4565, git-rebase-conflict-resolution).
    ///
    /// This test locks the floor contract so a future config change cannot silently
    /// lower it below the level that blocks fabricated matches for off-topic prompts.
    ///
    /// The prompt embedding `[1.0, 0.0]` and the skill embedding `[0.0, 1.0]` are
    /// orthogonal, giving cosine similarity = 0.0 (α term = 0). With the default
    /// weights (α=0.45, β=0.35, γ=0.20, λ=0.25) and no subunit evidence (β=0) and
    /// no prior (γ=0), the eq3 score is 0.0 — clearly below 0.450.
    /// The floor must exclude this candidate, leaving an empty candidates list.
    #[tokio::test]
    async fn relevance_floor_excludes_candidate_below_threshold() {
        use std::collections::BTreeMap;

        // Use the calibrated default threshold (0.450) as configured in RetrievalConfig.
        // A skill with zero cosine alignment gets eq3 = 0 — well below the floor.
        let floor_config = RetrievalConfig {
            candidate_limit: 10,
            max_results: 3,
            max_subunits_per_skill: 3,
            rescue_threshold: 0.15,
            relevance_threshold: 0.450, // calibrated floor from #192
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
            "candidate whose eq3 = 0 must be excluded by the 0.450 relevance floor; \
             got {} candidates (expected 0 — floor must block fabricated matches)",
            results[0].candidates.len()
        );
    }

    /// Proves that a candidate with strong subunit evidence that pushes eq3 above
    /// the relevance floor IS admitted, while the floor still blocks weaker candidates.
    ///
    /// This test demonstrates that a threshold of 0.450 does not over-reject:
    /// a skill with real subunit evidence (β > 0) clears the floor when its
    /// combined score α×0.45 + β×0.35 ≥ 0.450.
    ///
    /// With default weights and cosine=1.0 for both skill and subunit:
    ///   eq3 = 0.45 × 1.0 + 0.35 × 1.0 = 0.80 → well above 0.450.
    #[tokio::test]
    async fn relevance_floor_admits_candidate_with_sufficient_combined_score() {
        use std::collections::BTreeMap;

        let floor_config = RetrievalConfig {
            candidate_limit: 10,
            max_results: 3,
            max_subunits_per_skill: 3,
            rescue_threshold: 0.15,
            relevance_threshold: 0.450, // calibrated floor from #192
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
        // eq3 = 0.45 × 1.0 + 0.35 × 1.0 + 0.20 × 0.0 = 0.80 → above the 0.450 floor.
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
            "candidate with eq3 ≈ 0.80 must be admitted above the 0.450 relevance floor; \
             got {} candidates (expected 1)",
            results[0].candidates.len()
        );
        let admitted = &results[0].candidates[0];
        assert!(
            admitted.score >= 0.450,
            "admitted candidate must have score >= floor (0.450); got {:.4}",
            admitted.score
        );
    }
}
