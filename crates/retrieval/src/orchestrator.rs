use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
    sync::Arc,
    time::Instant,
};

use arc_swap::ArcSwap;
use async_trait::async_trait;
use domain::{
    EmbeddingError, EmbeddingService, ScopeDescriptor, ScopeType, ScoredSkill, Skill, Subunit,
    SubunitType,
};

use crate::{
    dual_scope::search_scopes_concurrently,
    fusion::{ScopeRanking, weighted_reciprocal_rank_fusion},
    scope_resolution::DualScopeResolver,
    scoring::ScoringWeights,
};

#[derive(Debug, Clone)]
pub struct SeededSkill {
    pub skill: Skill,
    pub scope_id: String,
    pub source_paths: Vec<PathBuf>,
    pub embedding: Vec<f32>,
    pub subunits: Vec<Subunit>,
    pub prior: f32,
    pub community_boost: f32,
}

/// Immutable in-memory snapshot of the skill graph the read path retrieves from.
///
/// `graph_version` is the durable `graph_state.graph_version` the snapshot was
/// built from; it keys the version-aware compiled-context cache so a rebuild
/// invalidates stale entries. An empty `skills` vector is a valid cold-start
/// snapshot and yields `no_match` rather than an error.
///
/// T02 wraps this type as `GraphSnapshot { graph, version }` under `ArcSwap` to
/// support refresh-without-restart; keep it free of swap/refresh concerns here.
#[derive(Debug, Clone)]
pub struct RetrievalSnapshot {
    pub graph_version: i64,
    pub skills: Vec<SeededSkill>,
}

impl RetrievalSnapshot {
    pub fn new(skills: Vec<SeededSkill>, graph_version: i64) -> Self {
        Self {
            graph_version,
            skills,
        }
    }
}

/// The atomically-swappable pair the read path consults.
///
/// `graph` and `version` are bound into one struct so a reader that takes a
/// single [`ArcSwap::load`] snapshot can never observe a graph from one rebuild
/// alongside the version from another. T02 (refresh-without-restart) swaps the
/// whole struct in one store; readers `load()` once and hold the resulting
/// [`arc_swap::Guard`] for the duration of a `retrieve` call, so an in-flight
/// retrieval completes against the graph it started on (the documented
/// in-flight safety invariant).
///
/// `version` mirrors `graph.graph_version`; it is duplicated here so the hot
/// path reads the version without dereferencing the (larger) snapshot when the
/// graph itself is not needed.
#[derive(Debug)]
pub struct GraphSnapshot {
    pub graph: Arc<RetrievalSnapshot>,
    pub version: i64,
}

impl GraphSnapshot {
    fn new(snapshot: RetrievalSnapshot) -> Self {
        let version = snapshot.graph_version;
        Self {
            graph: Arc::new(snapshot),
            version,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RetrievedSubunit {
    pub kind: SubunitType,
    pub title: String,
    pub content: String,
    pub relevance: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RetrievedSkill {
    pub scored_skill: ScoredSkill,
    pub highlights: Vec<RetrievedSubunit>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RescueCue {
    pub source_skill: String,
    pub title: String,
    pub content: String,
    pub relevance: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RetrievalOutcome {
    pub skills: Vec<RetrievedSkill>,
    pub rescue_pool: Vec<RescueCue>,
    pub degraded_scopes: Vec<String>,
    pub reason_codes: Vec<String>,
    pub health: BTreeMap<String, String>,
    pub scopes_considered: Vec<String>,
    pub graph_version: i64,
    pub latency_ms: u128,
}

impl RetrievalOutcome {
    pub fn is_degraded(&self) -> bool {
        !self.degraded_scopes.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct RetrievalConfig {
    pub scope_id: String,
    pub scope_type: ScopeType,
    pub candidate_limit: usize,
    pub max_results: usize,
    pub max_subunits_per_skill: usize,
    pub rescue_threshold: f32,
    pub relevance_threshold: f32,
    pub mmr_lambda: f32,
    pub scoring_weights: ScoringWeights,
    pub project_scope_weight: f32,
    pub global_scope_weight: f32,
    pub rrf_k: f32,
    pub scope_timeout_ms: u64,
}

impl Default for RetrievalConfig {
    fn default() -> Self {
        Self {
            scope_id: "global".to_owned(),
            scope_type: ScopeType::Global,
            candidate_limit: 50,
            max_results: 3,
            max_subunits_per_skill: 3,
            rescue_threshold: 0.15,
            relevance_threshold: 0.20,
            mmr_lambda: 0.65,
            scoring_weights: ScoringWeights::default(),
            project_scope_weight: 1.0,
            global_scope_weight: 0.7,
            rrf_k: 60.0,
            scope_timeout_ms: 400,
        }
    }
}

#[async_trait]
pub trait SkillRetriever: Send + Sync {
    async fn retrieve(&self, prompt: &str, repo_path: Option<&str>) -> RetrievalOutcome;
    fn current_graph_version(&self) -> i64;
    fn configured_scopes(&self) -> Vec<String>;
}

pub struct RetrievalOrchestrator<E>
where
    E: EmbeddingService + Send + Sync + 'static,
{
    embedding_service: Arc<E>,
    current: ArcSwap<GraphSnapshot>,
    config: RetrievalConfig,
    scope_resolver: Option<DualScopeResolver>,
}

impl<E> RetrievalOrchestrator<E>
where
    E: EmbeddingService + Send + Sync + 'static,
{
    pub fn new(
        embedding_service: Arc<E>,
        graph: RetrievalSnapshot,
        config: RetrievalConfig,
    ) -> Self {
        Self {
            embedding_service,
            current: ArcSwap::from_pointee(GraphSnapshot::new(graph)),
            config,
            scope_resolver: None,
        }
    }

    pub fn new_dual_scope(
        embedding_service: Arc<E>,
        graph: RetrievalSnapshot,
        config: RetrievalConfig,
        scope_resolver: DualScopeResolver,
    ) -> Self {
        Self {
            embedding_service,
            current: ArcSwap::from_pointee(GraphSnapshot::new(graph)),
            config,
            scope_resolver: Some(scope_resolver),
        }
    }

    /// Atomically replaces the in-memory read model with a freshly-loaded snapshot.
    ///
    /// This is the only refresh seam `retrieval` exposes (the Redis subscriber and
    /// the bounded Postgres reload live in `mcp-server`, keeping `retrieval`
    /// persistence- and transport-agnostic per ADR-0001).
    ///
    /// Concurrency contract:
    /// - The swap is a single lock-free `ArcSwap::rcu`; the closure may re-run under
    ///   contention, so `applied` is derived from the RETURNED previous Arc, not from
    ///   any closure-mutated state (which would be unsafe under re-execution).
    /// - Concurrent `retrieve` readers either see the entire old [`GraphSnapshot`] or
    ///   the entire new one, never a torn graph/version pair.
    /// - A `retrieve` call already holding the previous snapshot completes against
    ///   it (the in-flight safety invariant); only subsequent calls observe the new
    ///   graph.
    /// - Idempotent re-apply: replacing the current version with the same version is
    ///   a no-op, so a coalesced burst of `graph.rebuilt` events that resolves to an
    ///   already-applied version does not churn the read path.
    ///
    /// Returns `true` if the snapshot was applied (incoming version strictly newer),
    /// `false` if it was a no-op because the incoming version was not newer.
    pub fn swap_graph(&self, snapshot: RetrievalSnapshot) -> bool {
        let incoming_version = snapshot.graph_version;
        let new_snap = Arc::new(GraphSnapshot::new(snapshot));
        let prev = self.current.rcu(|current| {
            if incoming_version > current.version {
                Arc::clone(&new_snap)
            } else {
                Arc::clone(current)
            }
        });
        incoming_version > prev.version
    }

    /// Returns health markers for a fully operational read path.
    ///
    /// Keys reflect the actual read-path dependencies (Option A, ADR-0001):
    /// - `ollama`: embedding provider used to vectorize the prompt at query time.
    /// - `skill_snapshot_sync`: the in-memory CQRS read model; label reflects that
    ///   retrieval results are only as fresh as the last snapshot rebuild.
    /// - `filesystem_index`: lexical graph derived from the snapshot.
    ///
    /// Qdrant and Postgres are intentionally absent: Qdrant is a durable write-side
    /// store (graph-builder → outbox → Qdrant); it is NOT consulted at read time.
    /// Postgres is a write-side persistence layer. Listing either here would imply
    /// Qdrant/Postgres down ⇒ retrieval degraded, which is false under Option A.
    fn healthy_markers() -> BTreeMap<String, String> {
        BTreeMap::from([
            ("ollama".to_owned(), "ok".to_owned()),
            ("skill_snapshot_sync".to_owned(), "ok".to_owned()),
            ("filesystem_index".to_owned(), "ok".to_owned()),
        ])
    }

    /// Returns health markers for a degraded read path (e.g. embedding failure).
    ///
    /// Only the embedding provider is marked degraded; the CQRS read model
    /// (`skill_snapshot_sync`) and filesystem index remain independent. Qdrant and
    /// Postgres are excluded for the same reason as `healthy_markers`: they are
    /// write-side concerns invisible to the read path under Option A (ADR-0001).
    fn degraded_marker(reason: &str) -> BTreeMap<String, String> {
        BTreeMap::from([
            ("ollama".to_owned(), "degraded".to_owned()),
            ("skill_snapshot_sync".to_owned(), "ok".to_owned()),
            ("filesystem_index".to_owned(), "ok".to_owned()),
            ("reason".to_owned(), reason.to_owned()),
        ])
    }

    fn map_embedding_error_to_reason(error: &EmbeddingError) -> String {
        match error {
            EmbeddingError::ProviderUnavailable(_) => "embedding_provider_unavailable".to_owned(),
            EmbeddingError::Timeout { .. } => "embedding_timeout".to_owned(),
            EmbeddingError::InvalidInput(_) => "embedding_invalid_input".to_owned(),
            EmbeddingError::Unexpected(_) => "embedding_unexpected".to_owned(),
        }
    }

    async fn resolve_scopes(
        &self,
        repo_path: Option<&str>,
    ) -> (Vec<ScopeDescriptor>, Vec<String>, Vec<String>, Vec<String>) {
        if let Some(scope_resolver) = &self.scope_resolver {
            let outcome = scope_resolver.resolve(repo_path).await;
            (
                outcome.resolved_scopes(),
                outcome.scopes_considered(),
                outcome.degraded_scopes,
                outcome.reason_codes,
            )
        } else {
            (
                vec![ScopeDescriptor {
                    scope_id: self.config.scope_id.clone(),
                    scope_type: self.config.scope_type,
                    paths: Vec::new(),
                    config: BTreeMap::from([("resolver".to_owned(), "static".to_owned())]),
                }],
                vec![self.config.scope_id.clone()],
                Vec::new(),
                Vec::new(),
            )
        }
    }

    fn scope_weight(&self, scope_type: ScopeType) -> f32 {
        match scope_type {
            ScopeType::Project => self.config.project_scope_weight,
            ScopeType::Global => self.config.global_scope_weight,
            ScopeType::Team => self.config.global_scope_weight,
        }
    }

    fn dedupe(values: &mut Vec<String>) {
        let mut seen = HashSet::new();
        values.retain(|value| seen.insert(value.clone()));
    }

    fn build_degraded_outcome(
        &self,
        started: Instant,
        graph_version: i64,
        mut degraded_scopes: Vec<String>,
        mut reason_codes: Vec<String>,
        scopes_considered: Vec<String>,
    ) -> RetrievalOutcome {
        Self::dedupe(&mut degraded_scopes);
        Self::dedupe(&mut reason_codes);

        if degraded_scopes.is_empty() {
            degraded_scopes = scopes_considered.clone();
        }

        let reason = reason_codes
            .first()
            .cloned()
            .unwrap_or_else(|| "retrieval_degraded".to_owned());

        RetrievalOutcome {
            skills: Vec::new(),
            rescue_pool: Vec::new(),
            degraded_scopes,
            reason_codes,
            health: Self::degraded_marker(&reason),
            scopes_considered,
            graph_version,
            latency_ms: started.elapsed().as_millis(),
        }
    }
}

#[async_trait]
impl<E> SkillRetriever for RetrievalOrchestrator<E>
where
    E: EmbeddingService + Send + Sync + 'static,
{
    async fn retrieve(&self, prompt: &str, repo_path: Option<&str>) -> RetrievalOutcome {
        let started = Instant::now();
        // Load the swappable read model exactly once for this call so the graph
        // and its version can never skew, even if a `swap_graph` lands mid-call
        // (in-flight safety invariant, ADR-0001).
        let snapshot = self.current.load_full();
        let graph = snapshot.graph.clone();
        let graph_version = snapshot.version;

        let (scopes, scopes_considered, mut degraded_scopes, mut reason_codes) =
            self.resolve_scopes(repo_path).await;

        if scopes.is_empty() {
            return self.build_degraded_outcome(
                started,
                graph_version,
                degraded_scopes,
                reason_codes,
                scopes_considered,
            );
        }

        let prompt_embedding = match self.embedding_service.embed_text(prompt).await {
            Ok(embedding) => embedding,
            Err(error) => {
                reason_codes.push(Self::map_embedding_error_to_reason(&error));
                return self.build_degraded_outcome(
                    started,
                    graph_version,
                    scopes_considered.clone(),
                    reason_codes,
                    scopes_considered,
                );
            }
        };

        let (scope_results, scope_failures) = search_scopes_concurrently(
            prompt,
            &prompt_embedding,
            graph.clone(),
            &self.config,
            &scopes,
        )
        .await;

        for failure in scope_failures {
            degraded_scopes.push(failure.scope_id);
            reason_codes.push(failure.reason_code);
        }

        if scope_results.is_empty() {
            return self.build_degraded_outcome(
                started,
                graph_version,
                degraded_scopes,
                reason_codes,
                scopes_considered,
            );
        }

        let scope_rankings: Vec<ScopeRanking> = scope_results
            .into_iter()
            .map(|result| ScopeRanking {
                scope_id: result.scope_id,
                weight: self.scope_weight(result.scope_type),
                candidates: result.candidates,
            })
            .collect();

        let fusion_limit = scope_rankings
            .iter()
            .map(|ranking| ranking.candidates.len())
            .sum::<usize>()
            .max(self.config.max_results);

        let ranked_candidates =
            weighted_reciprocal_rank_fusion(&scope_rankings, self.config.rrf_k, fusion_limit);

        let selected_candidates: Vec<_> = ranked_candidates
            .iter()
            .take(self.config.max_results)
            .cloned()
            .collect();
        let selected_ids: HashSet<String> = selected_candidates
            .iter()
            .map(|candidate| candidate.skill_id.clone())
            .collect();

        let selected_skills: Vec<RetrievedSkill> = selected_candidates
            .into_iter()
            .filter_map(|candidate| {
                let seeded_skill = graph.skills.get(candidate.skill_index)?;

                let highlights = candidate
                    .highlights
                    .into_iter()
                    .filter(|projection| projection.relevance > 0.0)
                    .map(|projection| RetrievedSubunit {
                        kind: projection.subunit.kind,
                        title: projection.subunit.title,
                        content: projection.subunit.content,
                        relevance: projection.relevance,
                    })
                    .collect();

                Some(RetrievedSkill {
                    scored_skill: ScoredSkill {
                        skill: seeded_skill.skill.clone(),
                        score: candidate.score,
                        matched_scope: candidate.matched_scope,
                        rationale: vec![
                            format!("rrf={:.6}", candidate.score),
                            format!("semantic={:.3}", candidate.semantic_score),
                            format!("lexical={:.3}", candidate.lexical_score),
                        ],
                    },
                    highlights,
                })
            })
            .collect();

        let rescue_pool: Vec<RescueCue> = ranked_candidates
            .iter()
            .filter(|candidate| !selected_ids.contains(&candidate.skill_id))
            .flat_map(|candidate| {
                let skill_name = graph
                    .skills
                    .get(candidate.skill_index)
                    .map(|seeded| seeded.skill.name.clone())
                    .unwrap_or_else(|| "unknown-skill".to_owned());

                candidate
                    .highlights
                    .iter()
                    .filter(|projection| projection.relevance >= self.config.rescue_threshold)
                    .map(move |projection| RescueCue {
                        source_skill: skill_name.clone(),
                        title: projection.subunit.title.clone(),
                        content: projection.subunit.content.clone(),
                        relevance: projection.relevance,
                    })
            })
            .collect();

        Self::dedupe(&mut degraded_scopes);
        Self::dedupe(&mut reason_codes);

        let health = if degraded_scopes.is_empty() {
            Self::healthy_markers()
        } else {
            let reason = reason_codes
                .first()
                .cloned()
                .unwrap_or_else(|| "retrieval_degraded".to_owned());
            Self::degraded_marker(&reason)
        };

        RetrievalOutcome {
            skills: selected_skills,
            rescue_pool,
            degraded_scopes,
            reason_codes,
            health,
            scopes_considered,
            graph_version,
            latency_ms: started.elapsed().as_millis(),
        }
    }

    fn current_graph_version(&self) -> i64 {
        self.current.load().version
    }

    fn configured_scopes(&self) -> Vec<String> {
        if let Some(scope_resolver) = &self.scope_resolver {
            scope_resolver.configured_scope_ids()
        } else {
            vec![self.config.scope_id.clone()]
        }
    }
}

#[cfg(test)]
mod tests {
    use domain::{EmbeddingError, EmbeddingService};

    use super::*;

    /// Minimal test-only embedding stub. Used only to satisfy the generic bound on
    /// `RetrievalOrchestrator<E>` so we can call its pure associated functions without
    /// constructing a real embedding provider.
    struct NoOpEmbeddingService;

    #[async_trait::async_trait]
    impl EmbeddingService for NoOpEmbeddingService {
        async fn embed_text(&self, _text: &str) -> Result<Vec<f32>, EmbeddingError> {
            Err(EmbeddingError::ProviderUnavailable("no-op stub".to_owned()))
        }

        async fn embed_batch(&self, _texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
            Err(EmbeddingError::ProviderUnavailable("no-op stub".to_owned()))
        }
    }

    /// Asserts that read-path health markers do NOT claim Qdrant, Postgres, or Redis
    /// as live read dependencies. Option A (ratified in ADR-0001): the read path
    /// operates entirely on the in-memory `RetrievalSnapshot`; Qdrant is write-side
    /// only; Postgres and Redis are not consulted at query time.
    ///
    /// DS-003 contract: stopping Qdrant must NOT degrade `compile_context` — only the
    /// `qdrant_write_side` marker in the infrastructure health checker may change.
    /// This test is the deletion guard that prevents false write-side markers from
    /// reappearing on the read path.
    #[test]
    fn read_path_health_markers_do_not_claim_qdrant_postgres_or_redis_as_live_dependencies() {
        let healthy = RetrievalOrchestrator::<NoOpEmbeddingService>::healthy_markers();
        assert!(
            !healthy.contains_key("qdrant"),
            "healthy_markers must not include 'qdrant': Qdrant is write-side only (Option A, ADR-0001)"
        );
        assert!(
            !healthy.contains_key("postgres"),
            "healthy_markers must not include 'postgres': Postgres is not a read-path dependency"
        );
        assert!(
            !healthy.contains_key("redis"),
            "healthy_markers must not include 'redis': Redis is not a read-path dependency"
        );
        assert!(
            healthy.get("skill_snapshot_sync").map(String::as_str) == Some("ok"),
            "healthy_markers must report skill_snapshot_sync: ok to represent the CQRS read model"
        );

        let degraded =
            RetrievalOrchestrator::<NoOpEmbeddingService>::degraded_marker("embedding_timeout");
        assert!(
            !degraded.contains_key("qdrant"),
            "degraded_marker must not include 'qdrant': Qdrant is write-side only (Option A, ADR-0001)"
        );
        assert!(
            !degraded.contains_key("postgres"),
            "degraded_marker must not include 'postgres': Postgres is not a read-path dependency"
        );
        assert!(
            !degraded.contains_key("redis"),
            "degraded_marker must not include 'redis': Redis is not a read-path dependency"
        );
    }

    /// Always-succeeds embedding stub so `retrieve` exercises the full read path
    /// (not just the degraded short-circuit) during the concurrency test.
    struct ConstantEmbeddingService;

    #[async_trait::async_trait]
    impl EmbeddingService for ConstantEmbeddingService {
        async fn embed_text(&self, _text: &str) -> Result<Vec<f32>, EmbeddingError> {
            Ok(vec![1.0, 0.0, 0.0, 0.0])
        }

        async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
            Ok(texts.iter().map(|_| vec![1.0, 0.0, 0.0, 0.0]).collect())
        }
    }

    /// Builds a snapshot whose skill count equals its version, so a torn read
    /// (graph from version A, reported version B) is directly detectable: the
    /// number of skills must always equal the reported `graph_version`.
    fn versioned_snapshot(version: i64) -> RetrievalSnapshot {
        let skills = (0..version)
            .map(|index| SeededSkill {
                skill: Skill {
                    id: domain::DomainId::new_unchecked(format!("skill-{version}-{index}")),
                    name: format!("skill-{version}-{index}"),
                    description: "concurrency probe skill".to_owned(),
                    scope: ScopeType::Global,
                    status: domain::SkillStatus::Ready,
                    lifecycle: domain::LifecycleStatus::Active,
                    tags: vec!["probe".to_owned()],
                    subunit_ids: Vec::new(),
                    community_id: None,
                },
                scope_id: "global".to_owned(),
                source_paths: Vec::new(),
                embedding: vec![1.0, 0.0, 0.0, 0.0],
                subunits: Vec::new(),
                prior: 0.1,
                community_boost: 0.2,
            })
            .collect();
        RetrievalSnapshot::new(skills, version)
    }

    /// Proves `swap_graph` gives no torn reads: a concurrent reader either sees the
    /// whole old [`GraphSnapshot`] or the whole new one, so the reported version
    /// always matches the graph it was loaded with. Before `ArcSwap`/`swap_graph`
    /// existed the graph and version were independently mutable fields and could
    /// skew under concurrency; this guards that regression.
    #[tokio::test]
    async fn swap_graph_never_yields_torn_graph_version_under_concurrent_readers() {
        let orchestrator = Arc::new(RetrievalOrchestrator::new(
            Arc::new(ConstantEmbeddingService),
            versioned_snapshot(1),
            RetrievalConfig::default(),
        ));

        let writer = {
            let orchestrator = orchestrator.clone();
            tokio::spawn(async move {
                for version in 2..=200_i64 {
                    let applied = orchestrator.swap_graph(versioned_snapshot(version));
                    assert!(applied, "monotonic version {version} must apply");
                    tokio::task::yield_now().await;
                }
            })
        };

        let mut readers = Vec::new();
        for _ in 0..8 {
            let orchestrator = orchestrator.clone();
            readers.push(tokio::spawn(async move {
                for _ in 0..400 {
                    let snapshot = orchestrator.current.load_full();
                    assert_eq!(
                        snapshot.graph.skills.len() as i64,
                        snapshot.version,
                        "load() must return a consistent graph/version pair"
                    );
                    assert_eq!(
                        snapshot.graph.graph_version, snapshot.version,
                        "GraphSnapshot.version must mirror the inner snapshot version"
                    );

                    let outcome = orchestrator.retrieve("probe", None).await;
                    let reported = outcome.graph_version;
                    assert!(
                        (1..=200).contains(&reported),
                        "reported version must be a real applied version, got {reported}"
                    );
                    tokio::task::yield_now().await;
                }
            }));
        }

        writer.await.expect("writer task should not panic");
        for reader in readers {
            reader.await.expect("reader task should not panic");
        }

        assert_eq!(
            orchestrator.current_graph_version(),
            200,
            "final version should be the last applied swap"
        );
    }

    /// Proves concurrent writers converge: N tasks writing monotonically-increasing
    /// versions 1..=N result in the final stored version == N, which is the only
    /// guaranteed property regardless of scheduling (the rcu closure may re-run under
    /// contention but the monotonic guard ensures it always picks the highest seen).
    #[tokio::test]
    async fn concurrent_writers_converge_to_highest_version() {
        const N: i64 = 64;
        let orchestrator = Arc::new(RetrievalOrchestrator::new(
            Arc::new(ConstantEmbeddingService),
            versioned_snapshot(0),
            RetrievalConfig::default(),
        ));

        let mut writers = Vec::new();
        for version in 1..=N {
            let orchestrator = orchestrator.clone();
            writers.push(tokio::spawn(async move {
                orchestrator.swap_graph(versioned_snapshot(version))
            }));
        }
        for writer in writers {
            writer.await.expect("writer task should not panic");
        }

        assert_eq!(
            orchestrator.current_graph_version(),
            N,
            "after N concurrent writers with versions 1..=N the final version must be N"
        );
    }

    /// Idempotent re-apply: replaying an already-applied (or older) version is a
    /// no-op, so a coalesced burst of `graph.rebuilt` never regresses the graph.
    #[test]
    fn swap_graph_is_idempotent_for_same_or_older_version() {
        let orchestrator = RetrievalOrchestrator::new(
            Arc::new(ConstantEmbeddingService),
            versioned_snapshot(5),
            RetrievalConfig::default(),
        );

        assert!(
            !orchestrator.swap_graph(versioned_snapshot(5)),
            "re-applying the same version must be a no-op"
        );
        assert!(
            !orchestrator.swap_graph(versioned_snapshot(3)),
            "applying an older version must be a no-op"
        );
        assert_eq!(orchestrator.current_graph_version(), 5);

        assert!(
            orchestrator.swap_graph(versioned_snapshot(6)),
            "a strictly newer version must apply"
        );
        assert_eq!(orchestrator.current_graph_version(), 6);
    }
}
