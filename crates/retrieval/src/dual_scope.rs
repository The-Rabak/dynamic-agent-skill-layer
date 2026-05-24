use std::{collections::HashMap, future::Future, sync::Arc, time::Duration};

use domain::ScopeDescriptor;
use tokio::time::timeout;

use crate::{
    fusion::{FusedCandidate, mmr_select},
    graph_search::{GraphHit, search_graph},
    orchestrator::{RetrievalConfig, SeededGraph},
    qdrant_search::search_qdrant,
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
    graph: Arc<SeededGraph>,
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
    graph: Arc<SeededGraph>,
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
    graph: Arc<SeededGraph>,
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

    let qdrant_hits = search_qdrant(prompt_embedding, &scoped_embeddings, config.candidate_limit);
    let candidate_indices: Vec<usize> = qdrant_hits
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

    let graph_hits = search_graph(
        prompt,
        &skill_text,
        &skill_subunits,
        &candidate_indices,
        config.max_subunits_per_skill,
    );
    let graph_hits_by_skill: HashMap<usize, GraphHit> = graph_hits
        .into_iter()
        .map(|hit| (hit.skill_index, hit))
        .collect();

    let mut fused_candidates: Vec<FusedCandidate> = qdrant_hits
        .iter()
        .filter_map(|qdrant_hit| {
            let scoped_skill_index = *scoped_indices.get(qdrant_hit.skill_index)?;
            let seeded_skill = graph.skills.get(scoped_skill_index)?;
            let graph_hit = graph_hits_by_skill.get(&scoped_skill_index);
            let lexical_score = graph_hit.map_or(0.0, |hit| hit.lexical_score);
            let score = score_eq3(
                ScoreComponents {
                    l1_semantic: qdrant_hit.semantic_score,
                    l0_lexical: lexical_score,
                    prior: seeded_skill.prior,
                    community_boost: seeded_skill.community_boost,
                },
                config.scoring_weights,
            );

            Some(FusedCandidate {
                skill_index: scoped_skill_index,
                skill_id: seeded_skill.skill.id.as_str().to_owned(),
                matched_scope: scope.scope_type,
                score,
                semantic_score: qdrant_hit.semantic_score,
                lexical_score,
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

    fn graph() -> SeededGraph {
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

        SeededGraph::new(
            vec![
                SeededSkill {
                    skill: project.clone(),
                    scope_id: "project".to_owned(),
                    source_paths: vec![PathBuf::from("/workspace/project/src/auth.rs")],
                    embedding: vec![1.0, 1.0],
                    subunits: vec![Subunit {
                        id: DomainId::new_unchecked("project-sub"),
                        skill_id: project.id.clone(),
                        kind: SubunitType::Procedure,
                        title: "Project auth middleware".to_owned(),
                        content: "Trace middleware sequence".to_owned(),
                        lifecycle: LifecycleStatus::Active,
                    }],
                    prior: 0.1,
                    community_boost: 0.2,
                },
                SeededSkill {
                    skill: global.clone(),
                    scope_id: "global".to_owned(),
                    source_paths: vec![PathBuf::from("/workspace/global/docs/auth.md")],
                    embedding: vec![0.9, 1.0],
                    subunits: vec![Subunit {
                        id: DomainId::new_unchecked("global-sub"),
                        skill_id: global.id.clone(),
                        kind: SubunitType::Convention,
                        title: "Global auth checklist".to_owned(),
                        content: "Validate token lifetime".to_owned(),
                        lifecycle: LifecycleStatus::Active,
                    }],
                    prior: 0.1,
                    community_boost: 0.2,
                },
            ],
            3,
        )
    }

    fn heavy_graph(skills_per_scope: usize) -> SeededGraph {
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

        SeededGraph::new(skills, 7)
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

        assert!(
            dual_elapsed < project_elapsed + global_elapsed,
            "dual scope search should complete faster than sequential per-scope path: dual={dual_elapsed:?}, project={project_elapsed:?}, global={global_elapsed:?}"
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
        let graph = SeededGraph::new(
            vec![
                SeededSkill {
                    skill: project.clone(),
                    scope_id: "global".to_owned(),
                    source_paths: vec![PathBuf::from("/workspace/project/src/auth.rs")],
                    embedding: vec![1.0, 1.0],
                    subunits: vec![Subunit {
                        id: DomainId::new_unchecked("project-sub"),
                        skill_id: project.id.clone(),
                        kind: SubunitType::Procedure,
                        title: "Project auth middleware".to_owned(),
                        content: "Trace middleware sequence".to_owned(),
                        lifecycle: LifecycleStatus::Active,
                    }],
                    prior: 0.1,
                    community_boost: 0.2,
                },
                SeededSkill {
                    skill: project,
                    scope_id: "project".to_owned(),
                    source_paths: vec![PathBuf::from("/outside-scope/auth.rs")],
                    embedding: vec![0.95, 1.0],
                    subunits: vec![Subunit {
                        id: DomainId::new_unchecked("project-sub-outside"),
                        skill_id: DomainId::new_unchecked("project-skill"),
                        kind: SubunitType::Procedure,
                        title: "Outside scope auth".to_owned(),
                        content: "Should be excluded".to_owned(),
                        lifecycle: LifecycleStatus::Active,
                    }],
                    prior: 0.1,
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
}
