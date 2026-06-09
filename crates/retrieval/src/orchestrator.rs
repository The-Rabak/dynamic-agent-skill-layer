use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use crate::bm25::Bm25Index;

use arc_swap::ArcSwap;
use async_trait::async_trait;
use domain::{
    EmbeddingError, EmbeddingService, ScopeDescriptor, ScopeType, ScoredSkill, Skill, Subunit,
    SubunitType,
};

use crate::{
    CircuitBreaker,
    dual_scope::{
        ScopedSearchResult, search_scopes_concurrently, search_scopes_with_qdrant_candidates,
    },
    fusion::{ScopeRanking, weighted_reciprocal_rank_fusion},
    hybrid::{HybridCandidateSource, HybridQueryError},
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
    /// One embedding per entry in `subunits`, in the same order. Used to score the
    /// β (`subunit_evidence`) term of eq.3 by cosine against the query embedding at
    /// request time (issue #172). Empty when a skill has no subunits.
    pub subunit_embeddings: Vec<Vec<f32>>,
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
///
/// `bm25_index` is an optional pre-built Okapi BM25 lexical index over the skill
/// corpus (T04-B). It is built unconditionally at snapshot construction time by
/// `build_graph_from_pg` so switching `RETRIEVAL_BACKEND` from dense to hybrid
/// requires no graph rebuild — the index is always present when the corpus is
/// non-empty. `None` only on cold-start snapshots (empty skill set) and in tests
/// that do not exercise the hybrid arm.
///
/// How the eq.3 `community_boost` (the λ term) is computed at query time (#208).
///
/// `Binary` is the historical behaviour (a uniform 0.2 for any community member,
/// which the #210 sweep proved is ranking-inert and mildly harmful). The other
/// two arms exist to settle the keep-or-cut decision on measured numbers, not
/// intuition. Selectable on the real server via `RETRIEVAL_COMMUNITY_BOOST_MODE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CommunityBoostMode {
    /// `0.2` for any skill in ≥1 community, else `0.0` (uniform → cancels out).
    #[default]
    Binary,
    /// Query-dependent: cosine(query, the skill's community centroid), clamped to
    /// `[0, 1]`. Boosts skills whose community is on-topic for THIS query.
    CentroidAffinity,
    /// No community boost (`0.0`), equivalent to λ=0. The graph does not touch ranking.
    Off,
}

impl std::str::FromStr for CommunityBoostMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "binary" => Ok(Self::Binary),
            "centroid_affinity" | "centroid" | "affinity" => Ok(Self::CentroidAffinity),
            "off" | "none" | "disabled" => Ok(Self::Off),
            other => Err(format!(
                "invalid CommunityBoostMode {other:?} (expected binary|centroid_affinity|off)"
            )),
        }
    }
}

/// Selects the candidate-generation backend for the retrieval read path.
///
/// `SnapshotDense` is the current default: cosine search over the in-memory
/// `RetrievalSnapshot`. `SnapshotHybrid` (T04-B) is the measured default hybrid
/// arm: it expands the dense candidate pool with BM25-scored skills so exact lexical
/// terms (tool names, crate names, API identifiers, file formats, invariants) surface
/// even when dense cosine blurs them. `QdrantHybrid` is implemented in sub-unit C.
///
/// Configurable via `RETRIEVAL_BACKEND` on the real server (parsed fail-loud by
/// `env_or` — a present-but-unparseable value panics rather than silently
/// defaulting, per the #243 requirement).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RetrievalBackend {
    /// In-memory cosine search over the `RetrievalSnapshot` (current default).
    #[default]
    SnapshotDense,
    /// In-memory BM25 + dense RRF over the `RetrievalSnapshot` (sub-unit B).
    SnapshotHybrid,
    /// Qdrant request-time dense + sparse fusion (sub-unit C, experimental).
    QdrantHybrid,
}

impl std::str::FromStr for RetrievalBackend {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "snapshot_dense" | "dense" => Ok(Self::SnapshotDense),
            "snapshot_hybrid" | "hybrid" => Ok(Self::SnapshotHybrid),
            "qdrant_hybrid" | "qdrant" => Ok(Self::QdrantHybrid),
            other => Err(format!(
                "invalid RetrievalBackend {other:?} (expected snapshot_dense|snapshot_hybrid|qdrant_hybrid)"
            )),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RetrievalSnapshot {
    pub graph_version: i64,
    pub skills: Vec<SeededSkill>,
    /// Per-community normalized centroid (mean of member ℓ₁ embeddings), keyed by
    /// community id. Populated by the real read-path loader (`build_graph_from_pg`)
    /// and consumed only by `CommunityBoostMode::CentroidAffinity`. Empty for the
    /// binary/off modes and for test snapshots, which is harmless (lookup misses
    /// → boost 0.0).
    pub community_centroids: HashMap<String, Vec<f32>>,
    /// Pre-built Okapi BM25 index over the skill corpus for the `SnapshotHybrid`
    /// arm (T04-B). Built unconditionally at snapshot construction by
    /// `build_graph_from_pg` — including an empty-but-valid index for an empty
    /// corpus (#247) — so the backend can switch dense↔hybrid at request time
    /// without a graph rebuild, and so the hybrid arm never observes `None` in
    /// production. `None` only for test snapshots that do not exercise the hybrid
    /// arm; reaching `SnapshotHybrid` with `None` is a fail-loud programming error.
    pub bm25_index: Option<Arc<Bm25Index>>,
    /// Stable skill id → index into `skills`, built once at construction so the
    /// `qdrant_hybrid` read path can map Qdrant candidates back to snapshot rows
    /// in O(1) instead of an O(k×N) linear scan per request (#254). First index
    /// wins on a duplicate id, matching the prior `iter().find()` semantics.
    pub skill_id_to_index: HashMap<String, usize>,
}

impl RetrievalSnapshot {
    pub fn new(skills: Vec<SeededSkill>, graph_version: i64) -> Self {
        // Derived purely from `skills`, so it stays consistent for every snapshot
        // (tests included) without a separate builder step that could drift.
        let mut skill_id_to_index = HashMap::with_capacity(skills.len());
        for (idx, seeded) in skills.iter().enumerate() {
            skill_id_to_index
                .entry(seeded.skill.id.as_str().to_owned())
                .or_insert(idx);
        }
        Self {
            graph_version,
            skills,
            community_centroids: HashMap::new(),
            bm25_index: None,
            skill_id_to_index,
        }
    }

    /// Attaches per-community centroids (builder style) for the centroid-affinity
    /// community-boost arm (#208).
    pub fn with_community_centroids(mut self, centroids: HashMap<String, Vec<f32>>) -> Self {
        self.community_centroids = centroids;
        self
    }

    /// Attaches a pre-built BM25 index (builder style) for the `SnapshotHybrid`
    /// retrieval arm (T04-B). The index is `Arc`-wrapped so the snapshot clone cost
    /// is a single atomic reference count increment — no corpus copy on each refresh.
    ///
    /// Called by `build_graph_from_pg` after the skill corpus is assembled so the
    /// snapshot carries the BM25 index from its first use. Tests that do not exercise
    /// the hybrid arm can skip this call; `bm25_index` defaults to `None`.
    pub fn with_bm25_index(mut self, index: Arc<Bm25Index>) -> Self {
        self.bm25_index = Some(index);
        self
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
    /// How the eq.3 community boost is computed (#208). Default `Off` after the
    /// measured keep-or-cut decision; `Binary` preserves the historical,
    /// ranking-inert behaviour and `CentroidAffinity` remains for re-evaluation.
    pub community_boost_mode: CommunityBoostMode,
    /// Candidate-generation backend. Default `SnapshotDense` (current behavior).
    /// `SnapshotHybrid` and `QdrantHybrid` are wired in sub-units B and C.
    /// Set via `RETRIEVAL_BACKEND`; a present-but-unparseable value panics (#243).
    pub backend: RetrievalBackend,
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
            // No-match relevance floor — RECALIBRATED on the real 234-corpus (#209,
            // 2026-06-08), superseding the original 8-skill toy calibration (#192).
            //
            // The original 0.450 was calibrated on an isolated 8-skill corpus with a
            // razor-thin 0.0179 bimodal gap. Re-measured on the real 234-skill corpus by
            // sweeping the floor on the LIVE mcp-server (find_skill over HTTP + real
            // claude judge; 40 tuning positives + 20 off-topic negatives), calibrating on
            // tuning and validating on the disjoint held-out split:
            //
            //   floor   no_match precision   pos hit@3   pos MRR   (tuning, judge-aug)
            //   0.45        0.600              0.725       0.533    (the old floor: leaks)
            //   0.46        0.800              0.675       0.596
            //   0.48        1.000              0.800       0.662   <-- chosen
            //   0.50        1.000              0.750       0.683
            //   0.52        1.000              0.725       0.725
            //
            // Finding: the old 0.450 was MISCALIBRATED TOO LOW. On a heterogeneous corpus
            // it admits mediocre-eq3 skills that (via RRF) both displace better skills in
            // the returned top-k AND let off-topic queries fabricate matches (no_match
            // precision only 0.600 on the 20-negative set). Raising the floor to 0.48
            // improves BOTH negative rejection (→1.000) AND positive ranking — it is not
            // the usual precision/recall tradeoff because removing low-score noise cleans
            // the top-k. Held-out validation at 0.48: no_match precision 1.000, MRR 0.767,
            // hit@3 0.867, recall@3 0.808 (vs 0.644 MRR at the old 0.450). 0.48 is the
            // lowest floor reaching perfect no_match precision, preserving the most recall
            // headroom for unseen queries. See
            // docs/assessments/2026-06-07-retrieval-quality-234-corpus-measured.md.
            //
            // Env-overridable via `RETRIEVAL_RELEVANCE_THRESHOLD` for retuning without a
            // redeploy (and the #209 sweep itself).
            relevance_threshold: 0.48,
            mmr_lambda: 0.65,
            scoring_weights: ScoringWeights::default(),
            project_scope_weight: 1.0,
            global_scope_weight: 0.7,
            rrf_k: 60.0,
            scope_timeout_ms: 400,
            // #208 keep-or-cut DECISION (measured on the real 234-corpus, 2026-06-08):
            // the community graph is DEMOTED from ranking. Measured held-out
            // judge-augmented quality (find_skill, real claude judge):
            //   (a) binary 0.2:        MRR 0.594, nDCG@3 0.484, no_match prec 0.800
            //   (b) centroid_affinity: MRR 0.667, nDCG@3 0.530, no_match prec 0.600
            //   (c) off (this default):MRR 0.644, nDCG@3 0.556, no_match prec 1.000
            // (b)'s +0.022 MRR over (c) is within noise (1 query ≈ 0.033) and is
            // bought with a catastrophic no_match regression (1.000 → 0.600) and a
            // LOWER nDCG. (c) Off is robust-best on nDCG@3, hit@3, and no_match. The
            // HDBSCAN communities remain a build-time organizational/diagnostic
            // artifact but no longer touch retrieval ranking. CentroidAffinity is
            // retained behind RETRIEVAL_COMMUNITY_BOOST_MODE for future re-evaluation
            // (e.g. under a stronger embedding model). See
            // docs/assessments/2026-06-07-retrieval-quality-234-corpus-measured.md.
            community_boost_mode: CommunityBoostMode::Off,
            backend: RetrievalBackend::SnapshotDense,
        }
    }
}

impl RetrievalConfig {
    /// Reads `RETRIEVAL_RELEVANCE_THRESHOLD` from the environment and returns the
    /// parsed value, or the calibrated default (0.48) if the variable is absent.
    ///
    /// Panics with a clear message when the variable is present but cannot be
    /// parsed as `f32` — per the project fail-loud mandate (no silent fallbacks).
    ///
    /// The default was recalibrated on the live 234-corpus on 2026-06-08;
    /// see the `RetrievalConfig::default()` comment for the full evidence table.
    pub fn relevance_threshold_from_env() -> f32 {
        match std::env::var("RETRIEVAL_RELEVANCE_THRESHOLD") {
            Ok(raw) => raw.parse().unwrap_or_else(|_| {
                panic!(
                    "RETRIEVAL_RELEVANCE_THRESHOLD is set but not a valid f32: {:?}",
                    raw
                )
            }),
            Err(_) => RetrievalConfig::default().relevance_threshold,
        }
    }

    /// Builds a config from `default()`, overriding each ranking lever from its
    /// `RETRIEVAL_*` environment variable when present. Absent → the default.
    ///
    /// This is real operational tuning-without-redeploy (the same contract as
    /// [`relevance_threshold_from_env`]): every override is parsed fail-loud —
    /// a present-but-unparseable variable panics rather than silently falling
    /// back, per the no-silent-fallback mandate. The retrieval-quality sweep
    /// (#210) uses these to measure each lever on the REAL running server by
    /// rebooting it per config; no in-process reconstruction.
    ///
    /// Recognised variables: `RETRIEVAL_ALPHA`, `RETRIEVAL_BETA`,
    /// `RETRIEVAL_GAMMA`, `RETRIEVAL_LAMBDA`, `RETRIEVAL_MMR_LAMBDA`,
    /// `RETRIEVAL_CANDIDATE_LIMIT`, `RETRIEVAL_MAX_RESULTS`,
    /// `RETRIEVAL_MAX_SUBUNITS_PER_SKILL`, `RETRIEVAL_RESCUE_THRESHOLD`,
    /// `RETRIEVAL_RELEVANCE_THRESHOLD`, `RETRIEVAL_PROJECT_SCOPE_WEIGHT`,
    /// `RETRIEVAL_GLOBAL_SCOPE_WEIGHT`, `RETRIEVAL_RRF_K`,
    /// `RETRIEVAL_COMMUNITY_BOOST_MODE` (`binary`|`centroid_affinity`|`off`),
    /// `RETRIEVAL_BACKEND` (`snapshot_dense`|`snapshot_hybrid`|`qdrant_hybrid`).
    pub fn from_env() -> Self {
        let d = RetrievalConfig::default();
        Self {
            candidate_limit: env_or("RETRIEVAL_CANDIDATE_LIMIT", d.candidate_limit),
            max_results: env_or("RETRIEVAL_MAX_RESULTS", d.max_results),
            max_subunits_per_skill: env_or(
                "RETRIEVAL_MAX_SUBUNITS_PER_SKILL",
                d.max_subunits_per_skill,
            ),
            rescue_threshold: env_or("RETRIEVAL_RESCUE_THRESHOLD", d.rescue_threshold),
            relevance_threshold: env_or("RETRIEVAL_RELEVANCE_THRESHOLD", d.relevance_threshold),
            mmr_lambda: env_or("RETRIEVAL_MMR_LAMBDA", d.mmr_lambda),
            scoring_weights: ScoringWeights {
                alpha: env_or("RETRIEVAL_ALPHA", d.scoring_weights.alpha),
                beta: env_or("RETRIEVAL_BETA", d.scoring_weights.beta),
                gamma: env_or("RETRIEVAL_GAMMA", d.scoring_weights.gamma),
                lambda: env_or("RETRIEVAL_LAMBDA", d.scoring_weights.lambda),
            },
            project_scope_weight: env_or("RETRIEVAL_PROJECT_SCOPE_WEIGHT", d.project_scope_weight),
            global_scope_weight: env_or("RETRIEVAL_GLOBAL_SCOPE_WEIGHT", d.global_scope_weight),
            rrf_k: env_or("RETRIEVAL_RRF_K", d.rrf_k),
            community_boost_mode: env_or("RETRIEVAL_COMMUNITY_BOOST_MODE", d.community_boost_mode),
            backend: env_or("RETRIEVAL_BACKEND", d.backend),
            ..d
        }
    }
}

/// Parses `name` from the environment as `T`, or returns `default` when absent.
/// Panics fail-loud when the variable is present but unparseable (no silent
/// fallback) — matching [`RetrievalConfig::relevance_threshold_from_env`].
fn env_or<T>(name: &str, default: T) -> T
where
    T: std::str::FromStr,
    <T as std::str::FromStr>::Err: std::fmt::Display,
{
    match std::env::var(name) {
        // An empty/whitespace value is treated as absent so a compose
        // `${VAR:-}` passthrough for an unset override falls back to the default
        // rather than tripping the fail-loud parse below.
        Ok(raw) if raw.trim().is_empty() => default,
        Ok(raw) => raw
            .trim()
            .parse()
            .unwrap_or_else(|e| panic!("{name} is set but not a valid value ({e}): {raw:?}")),
        Err(_) => default,
    }
}

#[async_trait]
pub trait SkillRetriever: Send + Sync {
    async fn retrieve(&self, prompt: &str, repo_path: Option<&str>) -> RetrievalOutcome;
    fn current_graph_version(&self) -> i64;
    fn configured_scopes(&self) -> Vec<String>;
}

/// Orchestrates the retrieval read path: resolves scopes, embeds the prompt via
/// the configured embedding provider, searches the in-memory skill graph, and
/// fuses results.
///
/// The `embedding_breaker` guards the per-request `embed_text` call.  Once it
/// trips (after `failure_threshold` consecutive failures) subsequent requests
/// short-circuit immediately and return a degraded outcome with
/// `reason: embedding_circuit_open`, so callers observe a loud, observable
/// degradation instead of eating the full provider timeout on every request.
///
/// Keep the breaker local to `retrieval`: CI enforces that this crate does not
/// depend on infrastructure adapters such as sqlx, redis, or qdrant.
pub struct RetrievalOrchestrator<E>
where
    E: EmbeddingService + Send + Sync + 'static,
{
    embedding_service: Arc<E>,
    current: ArcSwap<GraphSnapshot>,
    config: RetrievalConfig,
    scope_resolver: Option<DualScopeResolver>,
    /// Circuit breaker guarding the per-request embedding call.
    ///
    /// Closed = normal; Open = return degraded immediately without calling the
    /// provider; HalfOpen = allow one probe to test recovery.
    embedding_breaker: CircuitBreaker,
    /// Source for dense+sparse hybrid candidates from Qdrant at request time.
    ///
    /// `Some` only when `config.backend == RetrievalBackend::QdrantHybrid`.
    /// Injected by `mcp-server` at construction so the `retrieval` crate stays
    /// free of any direct `infrastructure` dependency.
    ///
    /// Absence under `QdrantHybrid` is treated as a fatal configuration error:
    /// `retrieve()` returns a loud degraded outcome with reason
    /// `qdrant_hybrid_source_absent` instead of silently falling back to dense.
    hybrid_candidate_source: Option<Arc<dyn HybridCandidateSource>>,
}

impl<E> RetrievalOrchestrator<E>
where
    E: EmbeddingService + Send + Sync + 'static,
{
    /// Builds a `CircuitBreaker` from environment variables, failing loudly if
    /// a present-but-malformed value is found (per the project no-stubs mandate).
    ///
    /// - `EMBED_CIRCUIT_FAILURE_THRESHOLD` — u32, defaults to `5`
    /// - `EMBED_CIRCUIT_OPEN_FOR_SECS` — u64 (seconds), defaults to `30`
    ///
    /// Panics with a clear message if either variable is set to a value that
    /// cannot be parsed.  Missing variables silently use the documented defaults.
    pub fn build_embedding_circuit_breaker_from_env() -> CircuitBreaker {
        let failure_threshold: u32 = match std::env::var("EMBED_CIRCUIT_FAILURE_THRESHOLD") {
            Ok(raw) => raw.parse().unwrap_or_else(|_| {
                panic!(
                    "EMBED_CIRCUIT_FAILURE_THRESHOLD is set but not a valid u32: {:?}",
                    raw
                )
            }),
            Err(_) => 5,
        };

        let open_for_secs: u64 = match std::env::var("EMBED_CIRCUIT_OPEN_FOR_SECS") {
            Ok(raw) => raw.parse().unwrap_or_else(|_| {
                panic!(
                    "EMBED_CIRCUIT_OPEN_FOR_SECS is set but not a valid u64: {:?}",
                    raw
                )
            }),
            Err(_) => 30,
        };

        CircuitBreaker::new(failure_threshold, Duration::from_secs(open_for_secs))
    }

    /// Constructs an orchestrator with a single static scope and a default
    /// embedding circuit breaker built from environment variables.
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
            embedding_breaker: Self::build_embedding_circuit_breaker_from_env(),
            hybrid_candidate_source: None,
        }
    }

    /// Constructs an orchestrator with dual-scope resolution and a
    /// caller-supplied embedding circuit breaker.
    ///
    /// The caller is responsible for building the breaker (typically via
    /// [`Self::build_embedding_circuit_breaker_from_env`]) so the production
    /// wiring site in `mcp-server` owns the breaker lifetime and can share or
    /// inspect it if needed.
    pub fn new_dual_scope(
        embedding_service: Arc<E>,
        graph: RetrievalSnapshot,
        config: RetrievalConfig,
        scope_resolver: DualScopeResolver,
        embedding_breaker: CircuitBreaker,
    ) -> Self {
        Self {
            embedding_service,
            current: ArcSwap::from_pointee(GraphSnapshot::new(graph)),
            config,
            scope_resolver: Some(scope_resolver),
            embedding_breaker,
            hybrid_candidate_source: None,
        }
    }

    /// Constructs an orchestrator with a caller-provided circuit breaker.
    ///
    /// Intended for tests and for callers that manage the breaker lifecycle
    /// externally (e.g. to share state or wire in a pre-tripped breaker for
    /// controlled degradation testing).
    pub fn new_with_breaker(
        embedding_service: Arc<E>,
        graph: RetrievalSnapshot,
        config: RetrievalConfig,
        embedding_breaker: CircuitBreaker,
    ) -> Self {
        Self {
            embedding_service,
            current: ArcSwap::from_pointee(GraphSnapshot::new(graph)),
            config,
            scope_resolver: None,
            embedding_breaker,
            hybrid_candidate_source: None,
        }
    }

    /// Attaches a `HybridCandidateSource` for the `QdrantHybrid` arm.
    ///
    /// Builder method: call after one of the standard constructors to wire the
    /// Qdrant read-path source when `RETRIEVAL_BACKEND=qdrant_hybrid`. When
    /// `backend != QdrantHybrid`, this method is a no-op (the source is set but
    /// never consulted). The production wiring site in `mcp-server` calls this
    /// only when the backend is `QdrantHybrid`.
    pub fn with_hybrid_candidate_source(mut self, source: Arc<dyn HybridCandidateSource>) -> Self {
        self.hybrid_candidate_source = Some(source);
        self
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
    /// Under `QdrantHybrid`, an additional `qdrant_hybrid_read` marker is included
    /// to make the Qdrant read-path dependency explicit. This intentionally breaks
    /// the CQRS "Qdrant-down cannot degrade compile_context" invariant for the
    /// `qdrant_hybrid` arm — T08 carries the ADR documenting this trade-off.
    ///
    /// Qdrant markers are ABSENT under `SnapshotDense` and `SnapshotHybrid`:
    /// those arms serve entirely from the in-memory snapshot and stopping Qdrant
    /// must NOT change their health output (DS-003 contract).
    fn healthy_markers_for_backend(backend: RetrievalBackend) -> BTreeMap<String, String> {
        let mut markers = BTreeMap::from([
            ("ollama".to_owned(), "ok".to_owned()),
            ("skill_snapshot_sync".to_owned(), "ok".to_owned()),
            ("filesystem_index".to_owned(), "ok".to_owned()),
        ]);
        if backend == RetrievalBackend::QdrantHybrid {
            markers.insert("qdrant_hybrid_read".to_owned(), "ok".to_owned());
        }
        markers
    }

    /// Returns health markers for a degraded read path (e.g. embedding failure).
    ///
    /// Only the embedding provider is marked degraded; the CQRS read model
    /// (`skill_snapshot_sync`) and filesystem index remain independent. Under
    /// `QdrantHybrid`, `qdrant_hybrid_read` is also included to reflect the
    /// degraded state of the live Qdrant read dependency.
    ///
    /// Qdrant markers are ABSENT for snapshot backends (same reasoning as
    /// `healthy_markers_for_backend`).
    fn degraded_marker_for_backend(
        backend: RetrievalBackend,
        reason: &str,
    ) -> BTreeMap<String, String> {
        let mut markers = BTreeMap::from([
            ("ollama".to_owned(), "degraded".to_owned()),
            ("skill_snapshot_sync".to_owned(), "ok".to_owned()),
            ("filesystem_index".to_owned(), "ok".to_owned()),
            ("reason".to_owned(), reason.to_owned()),
        ]);
        if backend == RetrievalBackend::QdrantHybrid {
            markers.insert("qdrant_hybrid_read".to_owned(), "degraded".to_owned());
        }
        markers
    }

    /// Reason code emitted when Qdrant is unreachable under the `QdrantHybrid` arm.
    ///
    /// Flows through `RetrievalOutcome.health["reason"]` so callers can
    /// distinguish Qdrant-down from embedding failure. No silent dense fallback.
    const REASON_QDRANT_HYBRID_UNAVAILABLE: &'static str = "qdrant_hybrid_unavailable";

    /// Reason code emitted when `QdrantHybrid` is configured but no
    /// `HybridCandidateSource` was injected at construction.
    ///
    /// This indicates a configuration/wiring error: the backend was set to
    /// `QdrantHybrid` but no Qdrant adapter was wired in. The operator must
    /// restart the server with a properly-wired `HybridCandidateSource`.
    const REASON_QDRANT_HYBRID_SOURCE_ABSENT: &'static str = "qdrant_hybrid_source_absent";

    /// Reason code emitted when the embedding circuit breaker is open.
    ///
    /// Flows through `RetrievalOutcome.health["reason"]` →
    /// `CompileContextResponse.health` so callers observe a loud, named
    /// degradation instead of a silent empty success.
    const REASON_EMBEDDING_CIRCUIT_OPEN: &'static str = "embedding_circuit_open";

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

    /// Executes the `QdrantHybrid` candidate-generation path.
    ///
    /// Queries Qdrant with the dense prompt embedding and a BM25 sparse query
    /// vector, maps the returned `HybridHit`s to snapshot skill indices via
    /// `skill_stable_id`, and returns per-scope `ScopedSearchResult`s ready for
    /// the existing eq.3 → floor → MMR pipeline.
    ///
    /// Returns `Err(HybridQueryError)` when the source is absent (configuration
    /// error) or when Qdrant returns an error (transport/status). The caller MUST
    /// fail loud and MUST NOT fall back to the snapshot-dense path.
    async fn search_scopes_qdrant_hybrid(
        &self,
        prompt: &str,
        prompt_embedding: &[f32],
        graph: Arc<RetrievalSnapshot>,
        scopes: &[domain::ScopeDescriptor],
    ) -> Result<
        (
            Vec<ScopedSearchResult>,
            Vec<crate::dual_scope::ScopedSearchFailure>,
        ),
        HybridQueryError,
    > {
        use crate::sparse::query_sparse_vector;

        let source = self.hybrid_candidate_source.as_ref().ok_or_else(|| {
            HybridQueryError::Transport(Self::REASON_QDRANT_HYBRID_SOURCE_ABSENT.to_owned())
        })?;

        // Build the sparse query vector from the prompt text.
        let (sparse_indices, sparse_values) = query_sparse_vector(prompt);

        // Query Qdrant for the top candidates (use candidate_limit as the Qdrant
        // limit; the snapshot-side eq.3 scoring and floor may further reduce this).
        let limit = self.config.candidate_limit as u64;
        let qdrant_candidates = source
            .query_hybrid(prompt_embedding, &sparse_indices, &sparse_values, limit)
            .await?;

        // Map each Qdrant candidate's stable id to its snapshot index via the
        // snapshot's precomputed `skill_id_to_index` (O(1) per candidate, built
        // once at construction — #254). Candidates with no matching snapshot row
        // are dropped (the snapshot is the source of truth for what is loaded).
        let fused_score_by_index: HashMap<usize, f32> = qdrant_candidates
            .iter()
            .filter_map(|candidate| {
                graph
                    .skill_id_to_index
                    .get(candidate.skill_stable_id.as_str())
                    .map(|&idx| (idx, candidate.fused_score))
            })
            .collect();

        Ok(search_scopes_with_qdrant_candidates(
            prompt,
            prompt_embedding,
            graph,
            &self.config,
            scopes,
            &fused_score_by_index,
        )
        .await)
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
            health: Self::degraded_marker_for_backend(self.config.backend, &reason),
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

        // Gate the embedding call behind the circuit breaker.  When the breaker
        // is open the provider has been repeatedly failing; skip the network call
        // entirely and return a loud degraded outcome with a named reason code so
        // callers can distinguish circuit-open from a transient embed failure.
        //
        // Manual allow/record (not `execute_with_resilience`) because
        // OllamaEmbeddingService already carries internal timeouts and the embed
        // batch path uses `retry_with_backoff`; stacking a second retry layer
        // here would violate the project's no-double-retry rule.
        let prompt_embedding = if !self.embedding_breaker.allow_request().await {
            reason_codes.push(Self::REASON_EMBEDDING_CIRCUIT_OPEN.to_owned());
            return self.build_degraded_outcome(
                started,
                graph_version,
                scopes_considered.clone(),
                reason_codes,
                scopes_considered,
            );
        } else {
            match self.embedding_service.embed_text(prompt).await {
                Ok(embedding) => {
                    self.embedding_breaker.record_success().await;
                    embedding
                }
                Err(error) => {
                    self.embedding_breaker.record_failure().await;
                    reason_codes.push(Self::map_embedding_error_to_reason(&error));
                    return self.build_degraded_outcome(
                        started,
                        graph_version,
                        scopes_considered.clone(),
                        reason_codes,
                        scopes_considered,
                    );
                }
            }
        };

        // Dispatch to the configured candidate-generation backend.
        //
        // - `SnapshotDense`: cosine search over the in-memory snapshot (current default).
        // - `SnapshotHybrid`: dense cosine + BM25 pool expansion + eq.3 scoring (T04-B).
        //   The hybrid arm expands the candidate pool with BM25-scored skills, then
        //   all candidates pass through the existing eq.3 scoring and relevance floor.
        // - `QdrantHybrid`: async Qdrant dense+sparse query; hits are mapped to snapshot
        //   skill indices via `skill_stable_id`, then run through eq.3 → floor → MMR
        //   exactly like the snapshot arms. Fail loud on Qdrant down — NO silent fallback
        //   to dense. The CQRS "Qdrant-down cannot degrade compile_context" contract is
        //   intentionally broken for this arm (T08 ADR).
        let (scope_results, scope_failures) = match self.config.backend {
            RetrievalBackend::SnapshotDense | RetrievalBackend::SnapshotHybrid => {
                search_scopes_concurrently(
                    prompt,
                    &prompt_embedding,
                    graph.clone(),
                    &self.config,
                    &scopes,
                )
                .await
            }
            RetrievalBackend::QdrantHybrid => {
                match self
                    .search_scopes_qdrant_hybrid(prompt, &prompt_embedding, graph.clone(), &scopes)
                    .await
                {
                    Ok(results) => results,
                    Err(qdrant_error) => {
                        // Qdrant-down or query error: fail loud.
                        // Do NOT silently fall back to snapshot_dense — that would
                        // mislabel the arm and violate the no-fakes mandate (#243).
                        reason_codes.push(Self::REASON_QDRANT_HYBRID_UNAVAILABLE.to_owned());
                        reason_codes.push(qdrant_error.to_string());
                        return self.build_degraded_outcome(
                            started,
                            graph_version,
                            scopes_considered.clone(),
                            reason_codes,
                            scopes_considered,
                        );
                    }
                }
            }
        };

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
                            format!("subunit_evidence={:.3}", candidate.subunit_evidence),
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
            Self::healthy_markers_for_backend(self.config.backend)
        } else {
            let reason = reason_codes
                .first()
                .cloned()
                .unwrap_or_else(|| "retrieval_degraded".to_owned());
            Self::degraded_marker_for_backend(self.config.backend, &reason)
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

    /// #208 decision guard: the community graph is DEMOTED from ranking. The
    /// default must be `Off` so no near-constant community boost silently enters
    /// the default ranking path (binary 0.2 was inert and mildly harmful; the
    /// measured (a)/(b)/(c) comparison is in the default() comment + assessment).
    #[test]
    fn default_community_boost_mode_is_off_graph_demoted_from_ranking() {
        assert_eq!(
            RetrievalConfig::default().community_boost_mode,
            CommunityBoostMode::Off,
            "the #208 measurement demoted the community graph from ranking; default must be Off"
        );
    }

    #[test]
    fn community_boost_mode_parses_from_env_strings() {
        use std::str::FromStr;
        assert_eq!(
            CommunityBoostMode::from_str("binary").unwrap(),
            CommunityBoostMode::Binary
        );
        assert_eq!(
            CommunityBoostMode::from_str("centroid_affinity").unwrap(),
            CommunityBoostMode::CentroidAffinity
        );
        assert_eq!(
            CommunityBoostMode::from_str("OFF").unwrap(),
            CommunityBoostMode::Off
        );
        assert!(CommunityBoostMode::from_str("nonsense").is_err());
    }

    /// T04-A guard: `RetrievalBackend` parses all canonical env values and
    /// rejects unknown strings with `Err` (which `env_or` promotes to a panic
    /// on the real server — the #243 fail-loud requirement).
    ///
    /// Aliases (`dense`, `hybrid`, `qdrant`) are also accepted so compose
    /// overrides stay terse. Default (absent/empty env) must be `SnapshotDense`
    /// so existing retrieval behavior is unchanged.
    #[test]
    fn retrieval_backend_parses_from_env_strings() {
        use std::str::FromStr;
        // Primary names.
        assert_eq!(
            "snapshot_dense".parse::<RetrievalBackend>().unwrap(),
            RetrievalBackend::SnapshotDense
        );
        assert_eq!(
            "snapshot_hybrid".parse::<RetrievalBackend>().unwrap(),
            RetrievalBackend::SnapshotHybrid
        );
        assert_eq!(
            "qdrant_hybrid".parse::<RetrievalBackend>().unwrap(),
            RetrievalBackend::QdrantHybrid
        );
        // Short aliases.
        assert_eq!(
            "dense".parse::<RetrievalBackend>().unwrap(),
            RetrievalBackend::SnapshotDense
        );
        assert_eq!(
            "hybrid".parse::<RetrievalBackend>().unwrap(),
            RetrievalBackend::SnapshotHybrid
        );
        assert_eq!(
            "qdrant".parse::<RetrievalBackend>().unwrap(),
            RetrievalBackend::QdrantHybrid
        );
        // Case-insensitive.
        assert_eq!(
            "SNAPSHOT_DENSE".parse::<RetrievalBackend>().unwrap(),
            RetrievalBackend::SnapshotDense
        );
        // Default.
        assert_eq!(
            RetrievalBackend::default(),
            RetrievalBackend::SnapshotDense,
            "default must be SnapshotDense so existing retrieval behavior is unchanged"
        );
        // Unknown value → Err (env_or promotes to panic on the real server).
        assert!(
            RetrievalBackend::from_str("bogus").is_err(),
            "unknown backend string must return Err, not silently default"
        );
    }

    /// T04-A config guard: `RetrievalConfig::default()` must carry `SnapshotDense`
    /// so absent-env deployments keep the current retrieval behavior unchanged.
    #[test]
    fn default_retrieval_backend_is_snapshot_dense() {
        assert_eq!(
            RetrievalConfig::default().backend,
            RetrievalBackend::SnapshotDense,
            "absent RETRIEVAL_BACKEND must default to SnapshotDense (no behavior change)"
        );
    }

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
    /// as live read dependencies for the snapshot backends. Option A (ratified in ADR-0001):
    /// the read path for SnapshotDense/SnapshotHybrid operates entirely on the in-memory
    /// `RetrievalSnapshot`; Qdrant is write-side only for those arms.
    ///
    /// DS-003 contract: stopping Qdrant must NOT degrade `compile_context` for snapshot
    /// backends — only the `qdrant_write_side` marker in the infrastructure health checker
    /// may change. This test is the deletion guard that prevents false write-side markers
    /// from reappearing on the snapshot read paths.
    ///
    /// Note: `QdrantHybrid` intentionally ADDS a `qdrant_hybrid_read` marker (tested
    /// separately in `qdrant_hybrid_health_marker_present_only_for_qdrant_backend`).
    #[test]
    fn read_path_health_markers_do_not_claim_qdrant_postgres_or_redis_as_live_dependencies() {
        // Snapshot arms must NOT include any Qdrant marker.
        for backend in [
            RetrievalBackend::SnapshotDense,
            RetrievalBackend::SnapshotHybrid,
        ] {
            let healthy =
                RetrievalOrchestrator::<NoOpEmbeddingService>::healthy_markers_for_backend(backend);
            assert!(
                !healthy.contains_key("qdrant"),
                "{backend:?}: healthy_markers must not include 'qdrant': Qdrant is write-side only (Option A, ADR-0001)"
            );
            assert!(
                !healthy.contains_key("qdrant_hybrid_read"),
                "{backend:?}: healthy_markers must not include 'qdrant_hybrid_read' for snapshot arms"
            );
            assert!(
                !healthy.contains_key("postgres"),
                "{backend:?}: healthy_markers must not include 'postgres'"
            );
            assert!(
                !healthy.contains_key("redis"),
                "{backend:?}: healthy_markers must not include 'redis'"
            );
            assert!(
                healthy.get("skill_snapshot_sync").map(String::as_str) == Some("ok"),
                "{backend:?}: healthy_markers must report skill_snapshot_sync: ok"
            );

            let degraded =
                RetrievalOrchestrator::<NoOpEmbeddingService>::degraded_marker_for_backend(
                    backend,
                    "embedding_timeout",
                );
            assert!(
                !degraded.contains_key("qdrant"),
                "{backend:?}: degraded_marker must not include 'qdrant'"
            );
            assert!(
                !degraded.contains_key("qdrant_hybrid_read"),
                "{backend:?}: degraded_marker must not include 'qdrant_hybrid_read' for snapshot arms"
            );
            assert!(
                !degraded.contains_key("postgres"),
                "{backend:?}: degraded_marker must not include 'postgres'"
            );
            assert!(
                !degraded.contains_key("redis"),
                "{backend:?}: degraded_marker must not include 'redis'"
            );
        }
    }

    /// Proves `qdrant_hybrid_read` marker is present ONLY under the `QdrantHybrid` backend.
    ///
    /// This is the companion to the snapshot-arm guard above: `qdrant_hybrid_read` MUST
    /// appear in healthy/degraded markers when the backend is `QdrantHybrid` (makes the
    /// Qdrant read-path dependency explicit), and MUST NOT appear for snapshot backends
    /// (preserves DS-003: Qdrant-down cannot degrade compile_context for snapshot arms).
    #[test]
    fn qdrant_hybrid_health_marker_present_only_for_qdrant_backend() {
        // QdrantHybrid: qdrant_hybrid_read must be present.
        let healthy = RetrievalOrchestrator::<NoOpEmbeddingService>::healthy_markers_for_backend(
            RetrievalBackend::QdrantHybrid,
        );
        assert!(
            healthy.contains_key("qdrant_hybrid_read"),
            "QdrantHybrid healthy_markers must include 'qdrant_hybrid_read' to expose the read dependency"
        );
        assert_eq!(
            healthy.get("qdrant_hybrid_read").map(String::as_str),
            Some("ok"),
            "QdrantHybrid healthy_markers: qdrant_hybrid_read must be 'ok'"
        );

        let degraded = RetrievalOrchestrator::<NoOpEmbeddingService>::degraded_marker_for_backend(
            RetrievalBackend::QdrantHybrid,
            "qdrant_hybrid_unavailable",
        );
        assert!(
            degraded.contains_key("qdrant_hybrid_read"),
            "QdrantHybrid degraded_marker must include 'qdrant_hybrid_read'"
        );
        assert_eq!(
            degraded.get("qdrant_hybrid_read").map(String::as_str),
            Some("degraded"),
            "QdrantHybrid degraded_marker: qdrant_hybrid_read must be 'degraded'"
        );

        // Snapshot arms must NOT have the qdrant_hybrid_read marker (tested above,
        // but recheck here as belt-and-suspenders for the exact QdrantHybrid isolation).
        for backend in [
            RetrievalBackend::SnapshotDense,
            RetrievalBackend::SnapshotHybrid,
        ] {
            let healthy =
                RetrievalOrchestrator::<NoOpEmbeddingService>::healthy_markers_for_backend(backend);
            assert!(
                !healthy.contains_key("qdrant_hybrid_read"),
                "{backend:?} must NOT have qdrant_hybrid_read in healthy_markers"
            );
        }
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
                subunit_embeddings: Vec::new(),
                // Prior is computed dynamically from real usage at graph-load
                // time (mcp-server lib.rs via `retrieval::usage_prior`). Test
                // fixtures use 0.0 (cold-start, no usage history) — the same
                // value `usage_prior(0, 0)` produces.
                prior: 0.0,
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

    // ── Circuit-breaker tests (issue #171) ───────────────────────────────────

    /// Tracks how many times `embed_text` was actually invoked.
    ///
    /// Always fails with `ProviderUnavailable` so the circuit breaker trips after
    /// `failure_threshold` calls. The invocation counter lets tests assert that no
    /// network call reaches the embedder while the breaker is open.
    struct CountingFailEmbeddingService {
        call_count: Arc<std::sync::atomic::AtomicU32>,
    }

    impl CountingFailEmbeddingService {
        fn new() -> (Arc<std::sync::atomic::AtomicU32>, Self) {
            let counter = Arc::new(std::sync::atomic::AtomicU32::new(0));
            (
                counter.clone(),
                Self {
                    call_count: counter,
                },
            )
        }
    }

    #[async_trait::async_trait]
    impl EmbeddingService for CountingFailEmbeddingService {
        async fn embed_text(&self, _text: &str) -> Result<Vec<f32>, EmbeddingError> {
            self.call_count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(EmbeddingError::ProviderUnavailable(
                "injected failure".to_owned(),
            ))
        }

        async fn embed_batch(&self, _texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
            Err(EmbeddingError::ProviderUnavailable(
                "injected failure".to_owned(),
            ))
        }
    }

    /// After `failure_threshold` consecutive embed failures the breaker opens:
    ///
    /// 1. Each failure is visible as a degraded outcome with the specific
    ///    `embedding_provider_unavailable` reason (breaker still closed — normal
    ///    degraded path).
    /// 2. Once open, the next call returns a degraded outcome with
    ///    `reason: embedding_circuit_open` WITHOUT invoking the embedder (call
    ///    count stays at `failure_threshold`).
    /// 3. Open-state health markers: `ollama: degraded`, `reason:
    ///    embedding_circuit_open` — a loud, observable degradation, not a silent
    ///    empty success.
    #[tokio::test]
    async fn embedding_circuit_breaker_trips_after_threshold_and_skips_embedder_while_open() {
        use std::time::Duration;

        const THRESHOLD: u32 = 3;
        let (call_count, embed_svc) = CountingFailEmbeddingService::new();
        let breaker = CircuitBreaker::new(THRESHOLD, Duration::from_secs(60));
        let orchestrator = RetrievalOrchestrator::new_with_breaker(
            Arc::new(embed_svc),
            versioned_snapshot(0),
            RetrievalConfig::default(),
            breaker,
        );

        // Drive the breaker to its threshold — each of these is a normal embed
        // failure (not circuit-open yet).
        for i in 1..=THRESHOLD {
            let outcome = orchestrator.retrieve("probe", None).await;
            assert!(
                outcome.is_degraded(),
                "call {i}: outcome must be degraded while breaker is still closed"
            );
            assert_eq!(
                outcome.health.get("ollama").map(String::as_str),
                Some("degraded"),
                "call {i}: ollama must be marked degraded"
            );
            assert_ne!(
                outcome.health.get("reason").map(String::as_str),
                Some("embedding_circuit_open"),
                "call {i}: reason must NOT be embedding_circuit_open while breaker is closed"
            );
        }

        // The embedder must have been called exactly THRESHOLD times.
        assert_eq!(
            call_count.load(std::sync::atomic::Ordering::SeqCst),
            THRESHOLD,
            "embedder must have been called exactly failure_threshold times before breaker opened"
        );

        // Breaker is now open.  The next call must NOT reach the embedder.
        let open_outcome = orchestrator.retrieve("probe-after-open", None).await;
        assert!(
            open_outcome.is_degraded(),
            "open-breaker call must produce a degraded outcome (not silent empty success)"
        );
        assert_eq!(
            open_outcome.health.get("ollama").map(String::as_str),
            Some("degraded"),
            "open-breaker: ollama must be marked degraded"
        );
        assert_eq!(
            open_outcome.health.get("reason").map(String::as_str),
            Some("embedding_circuit_open"),
            "open-breaker: reason must be embedding_circuit_open"
        );
        assert!(
            open_outcome
                .reason_codes
                .contains(&"embedding_circuit_open".to_owned()),
            "open-breaker: reason_codes must contain embedding_circuit_open"
        );

        // Critical: the embedder must NOT have been invoked again.
        assert_eq!(
            call_count.load(std::sync::atomic::Ordering::SeqCst),
            THRESHOLD,
            "embedder must NOT be called while the breaker is open (call count must stay at threshold)"
        );
    }

    /// After `open_for` elapses the breaker transitions to half-open, allows a
    /// single probe call, and a successful probe closes the breaker again.
    ///
    /// Uses a minimal `open_for` duration so the test is fast.
    #[tokio::test]
    async fn embedding_circuit_breaker_recovers_half_open_to_closed_after_open_for_elapses() {
        use crate::{CircuitBreaker, CircuitState};
        use std::time::Duration;

        let breaker = CircuitBreaker::new(1, Duration::from_millis(5));

        // Trip the breaker.
        breaker.record_failure().await;
        assert_eq!(
            breaker.state().await,
            CircuitState::Open,
            "breaker must be open after one failure with threshold=1"
        );

        // Wait for open_for to elapse.
        tokio::time::sleep(Duration::from_millis(10)).await;

        // A single success should close the breaker via the orchestrator path.
        // Use ConstantEmbeddingService so embed succeeds on the probe.
        let orchestrator = RetrievalOrchestrator::new_with_breaker(
            Arc::new(ConstantEmbeddingService),
            versioned_snapshot(0),
            RetrievalConfig::default(),
            breaker.clone(),
        );

        // Probe call — half-open allows one request.
        let probe_outcome = orchestrator.retrieve("probe", None).await;
        // An empty snapshot (version=0, no skills) returns not-degraded (all
        // scopes resolved, embed succeeded, just no results).  Verify the
        // breaker closed.
        let _ = probe_outcome; // outcome content is secondary here

        assert_eq!(
            breaker.state().await,
            CircuitState::Closed,
            "breaker must close after a successful probe during half-open"
        );
    }

    /// Proves `relevance_threshold_from_env` returns the calibrated default when
    /// `RETRIEVAL_RELEVANCE_THRESHOLD` is absent, and that the default is the
    /// calibrated 0.48 floor from #209.
    ///
    /// Recalibrated to 0.48 on the real 234-corpus (#209, 2026-06-08): the old 0.450
    /// (8-skill calibration) was too low — it leaked off-topic fabrications (no_match
    /// precision 0.600) and admitted ranking noise. 0.48 reaches perfect no_match
    /// precision AND higher positive quality on the live-server floor sweep.
    #[test]
    fn relevance_threshold_defaults_to_calibrated_floor() {
        // Temporarily remove the env var if it happens to be set in the test process.
        let _guard = EnvVarGuard::remove("RETRIEVAL_RELEVANCE_THRESHOLD");

        let threshold = RetrievalConfig::relevance_threshold_from_env();
        assert!(
            (threshold - 0.48).abs() < 1e-6,
            "calibrated default relevance_threshold must be 0.48 (#209 real-corpus recalibration); got {threshold:.6}"
        );
    }

    /// Proves `relevance_threshold_from_env` reads a valid override from the environment.
    #[test]
    fn relevance_threshold_reads_valid_env_override() {
        let _guard = EnvVarGuard::set("RETRIEVAL_RELEVANCE_THRESHOLD", "0.65");

        let threshold = RetrievalConfig::relevance_threshold_from_env();
        assert!(
            (threshold - 0.65).abs() < 1e-6,
            "env override must be respected; got {threshold:.6}"
        );
    }

    /// Scoped env-var helper for test isolation. Restores the original value on drop.
    struct EnvVarGuard {
        name: &'static str,
        original: Option<String>,
    }

    impl EnvVarGuard {
        fn remove(name: &'static str) -> Self {
            let original = std::env::var(name).ok();
            // SAFETY: test-only helper; tests run sequentially in their binary
            // (cargo test is single-threaded by default for unit tests in the same binary).
            unsafe { std::env::remove_var(name) };
            Self { name, original }
        }

        fn set(name: &'static str, value: &str) -> Self {
            let original = std::env::var(name).ok();
            // SAFETY: test-only helper; same rationale as above.
            unsafe { std::env::set_var(name, value) };
            Self { name, original }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.original {
                // SAFETY: test-only helper; same rationale as above.
                Some(val) => unsafe { std::env::set_var(self.name, val) },
                None => unsafe { std::env::remove_var(self.name) },
            }
        }
    }

    // ── T04-C3: QdrantHybrid arm unit tests ─────────────────────────────────

    use crate::hybrid::{HybridCandidate, HybridCandidateSource, HybridQueryError};

    /// Mock `HybridCandidateSource` that returns a fixed list of candidates.
    ///
    /// Used only in unit tests to inject Qdrant-like responses without a live Qdrant.
    struct FixedHybridSource {
        candidates: Vec<HybridCandidate>,
    }

    #[async_trait::async_trait]
    impl HybridCandidateSource for FixedHybridSource {
        async fn query_hybrid(
            &self,
            _dense: &[f32],
            _sparse_indices: &[u32],
            _sparse_values: &[f32],
            _limit: u64,
        ) -> Result<Vec<HybridCandidate>, HybridQueryError> {
            Ok(self.candidates.clone())
        }
    }

    /// Mock `HybridCandidateSource` that always returns a transport error.
    ///
    /// Used to test the fail-loud behavior when Qdrant is unreachable.
    struct FailingHybridSource;

    #[async_trait::async_trait]
    impl HybridCandidateSource for FailingHybridSource {
        async fn query_hybrid(
            &self,
            _dense: &[f32],
            _sparse_indices: &[u32],
            _sparse_values: &[f32],
            _limit: u64,
        ) -> Result<Vec<HybridCandidate>, HybridQueryError> {
            Err(HybridQueryError::Transport(
                "connection refused: Qdrant is down".to_owned(),
            ))
        }
    }

    /// Builds a two-skill snapshot for QdrantHybrid arm tests.
    ///
    /// Skill A: `global-skill-a` — embedding `[1.0, 0.0]` — strong semantic alignment
    ///   to a `[1.0, 0.0]` query.
    /// Skill B: `global-skill-b` — embedding `[0.0, 1.0]` — zero cosine with the
    ///   same query (used to test the relevance floor).
    fn qdrant_hybrid_snapshot() -> RetrievalSnapshot {
        use domain::{DomainId, LifecycleStatus, ScopeType, Skill, SkillStatus};

        let skill_a = Skill {
            id: DomainId::new_unchecked("global-skill-a"),
            name: "skill alpha".to_owned(),
            description: "alpha description with strong alignment".to_owned(),
            scope: ScopeType::Global,
            status: SkillStatus::Ready,
            lifecycle: LifecycleStatus::Active,
            tags: vec!["alpha".to_owned()],
            subunit_ids: vec![],
            community_id: None,
        };
        let skill_b = Skill {
            id: DomainId::new_unchecked("global-skill-b"),
            name: "skill beta orthogonal".to_owned(),
            description: "beta with zero cosine to the query".to_owned(),
            scope: ScopeType::Global,
            status: SkillStatus::Ready,
            lifecycle: LifecycleStatus::Active,
            tags: vec!["beta".to_owned()],
            subunit_ids: vec![],
            community_id: None,
        };
        RetrievalSnapshot::new(
            vec![
                SeededSkill {
                    skill: skill_a,
                    scope_id: "global".to_owned(),
                    source_paths: vec![],
                    // Match ConstantEmbeddingService's 4D output [1.0, 0.0, 0.0, 0.0]
                    embedding: vec![1.0, 0.0, 0.0, 0.0],
                    subunits: vec![],
                    subunit_embeddings: vec![],
                    prior: 0.0,
                    community_boost: 0.0,
                },
                SeededSkill {
                    skill: skill_b,
                    scope_id: "global".to_owned(),
                    source_paths: vec![],
                    // Orthogonal to [1.0, 0.0, 0.0, 0.0] → cosine = 0
                    embedding: vec![0.0, 1.0, 0.0, 0.0],
                    subunits: vec![],
                    subunit_embeddings: vec![],
                    prior: 0.0,
                    community_boost: 0.0,
                },
            ],
            1,
        )
    }

    /// Proves the `QdrantHybrid` arm maps Qdrant hits (by `skill_stable_id`) to
    /// snapshot skills and runs them through eq.3 → MMR, returning ranked results.
    ///
    /// Qdrant returns `global-skill-a` with a high fused score. The snapshot has
    /// skill A with a strong embedding alignment (`[1.0,0.0]` vs query `[1.0,0.0]`).
    /// eq.3 = α×1.0 + β×0 + γ×0 = 0.45 — above the low test floor of 0.1.
    #[tokio::test]
    async fn qdrant_hybrid_arm_maps_hits_to_snapshot_skills_and_ranks_via_eq3() {
        let source = Arc::new(FixedHybridSource {
            candidates: vec![HybridCandidate {
                skill_stable_id: "global-skill-a".to_owned(),
                fused_score: 0.85,
            }],
        });

        let config = RetrievalConfig {
            scope_id: "global".to_owned(),
            scope_type: domain::ScopeType::Global,
            candidate_limit: 10,
            max_results: 3,
            relevance_threshold: 0.1,
            backend: RetrievalBackend::QdrantHybrid,
            ..RetrievalConfig::default()
        };

        let orchestrator = RetrievalOrchestrator::new(
            Arc::new(ConstantEmbeddingService),
            qdrant_hybrid_snapshot(),
            config,
        )
        .with_hybrid_candidate_source(source);

        let outcome = orchestrator.retrieve("query alpha", None).await;

        assert!(
            !outcome.is_degraded(),
            "QdrantHybrid arm must return a non-degraded outcome when Qdrant responds: {:?}",
            outcome.health
        );
        assert_eq!(
            outcome.skills.len(),
            1,
            "exactly one skill must be returned (global-skill-a mapped from Qdrant hit)"
        );
        assert_eq!(
            outcome.skills[0].scored_skill.skill.id.as_str(),
            "global-skill-a",
            "the returned skill must be the one mapped from the Qdrant hit"
        );
        // qdrant_hybrid_read must be present in the healthy outcome.
        assert!(
            outcome.health.contains_key("qdrant_hybrid_read"),
            "QdrantHybrid healthy outcome must include qdrant_hybrid_read marker; got: {:?}",
            outcome.health
        );
        assert_eq!(
            outcome.health.get("qdrant_hybrid_read").map(String::as_str),
            Some("ok")
        );
    }

    /// Proves the relevance floor is authoritative over Qdrant-surfaced candidates:
    /// a skill returned by Qdrant whose eq.3 score falls below `relevance_threshold`
    /// is gated out and does NOT appear in the final result set.
    ///
    /// Skill B has embedding `[0.0,1.0]` (orthogonal to query `[1.0,0.0]`), so
    /// eq.3 = 0 — well below any positive floor. Even though Qdrant returned it,
    /// the floor must gate it out.
    #[tokio::test]
    async fn qdrant_hybrid_arm_relevance_floor_gates_low_eq3_qdrant_hit() {
        // Only return skill B (orthogonal embedding → eq.3 = 0).
        let source = Arc::new(FixedHybridSource {
            candidates: vec![HybridCandidate {
                skill_stable_id: "global-skill-b".to_owned(),
                fused_score: 0.99, // High Qdrant score but eq.3 will be 0
            }],
        });

        let config = RetrievalConfig {
            scope_id: "global".to_owned(),
            scope_type: domain::ScopeType::Global,
            candidate_limit: 10,
            max_results: 3,
            relevance_threshold: 0.48, // calibrated floor
            backend: RetrievalBackend::QdrantHybrid,
            ..RetrievalConfig::default()
        };

        let orchestrator = RetrievalOrchestrator::new(
            Arc::new(ConstantEmbeddingService),
            qdrant_hybrid_snapshot(),
            config,
        )
        .with_hybrid_candidate_source(source);

        let outcome = orchestrator.retrieve("query alpha", None).await;

        assert!(
            outcome.skills.is_empty(),
            "Qdrant-surfaced skill with eq.3=0 must be gated by the 0.48 floor; \
             got {} skills (floor is authoritative over Qdrant ranking)",
            outcome.skills.len()
        );
    }

    /// Proves that when Qdrant is unreachable under the `QdrantHybrid` arm, the
    /// orchestrator returns a loud degraded outcome with the
    /// `qdrant_hybrid_unavailable` reason — NOT a silent empty success or a
    /// fallback to the snapshot-dense path.
    #[tokio::test]
    async fn qdrant_hybrid_arm_fails_loud_when_qdrant_is_down() {
        let source = Arc::new(FailingHybridSource);

        let config = RetrievalConfig {
            scope_id: "global".to_owned(),
            scope_type: domain::ScopeType::Global,
            candidate_limit: 10,
            max_results: 3,
            relevance_threshold: 0.1,
            backend: RetrievalBackend::QdrantHybrid,
            ..RetrievalConfig::default()
        };

        let orchestrator = RetrievalOrchestrator::new(
            Arc::new(ConstantEmbeddingService),
            qdrant_hybrid_snapshot(),
            config,
        )
        .with_hybrid_candidate_source(source);

        let outcome = orchestrator.retrieve("query", None).await;

        assert!(
            outcome.is_degraded(),
            "QdrantHybrid arm must return degraded outcome when Qdrant is down"
        );
        assert!(
            outcome
                .reason_codes
                .contains(&"qdrant_hybrid_unavailable".to_owned()),
            "degraded reason_codes must include 'qdrant_hybrid_unavailable'; \
             got: {:?}",
            outcome.reason_codes
        );
        assert!(
            outcome.skills.is_empty(),
            "no skills must be returned when Qdrant is down under QdrantHybrid arm"
        );
        // Must NOT silently fall back to dense: the degraded marker must include
        // the qdrant_hybrid_read key to make the dependency visible.
        assert!(
            outcome.health.contains_key("qdrant_hybrid_read"),
            "degraded outcome must expose qdrant_hybrid_read in health markers"
        );
        assert_eq!(
            outcome.health.get("qdrant_hybrid_read").map(String::as_str),
            Some("degraded"),
            "qdrant_hybrid_read must be 'degraded' when Qdrant is down"
        );
    }

    /// Proves that configuring `QdrantHybrid` without injecting a
    /// `HybridCandidateSource` fails loud instead of panicking or silently
    /// degrading to dense.
    #[tokio::test]
    async fn qdrant_hybrid_arm_fails_loud_when_source_is_absent() {
        let config = RetrievalConfig {
            scope_id: "global".to_owned(),
            scope_type: domain::ScopeType::Global,
            candidate_limit: 10,
            max_results: 3,
            relevance_threshold: 0.1,
            backend: RetrievalBackend::QdrantHybrid,
            ..RetrievalConfig::default()
        };

        // No `.with_hybrid_candidate_source(...)` call — source is absent.
        let orchestrator = RetrievalOrchestrator::new(
            Arc::new(ConstantEmbeddingService),
            qdrant_hybrid_snapshot(),
            config,
        );

        let outcome = orchestrator.retrieve("query", None).await;

        assert!(
            outcome.is_degraded(),
            "QdrantHybrid without a source must return degraded, not empty success"
        );
        assert!(
            outcome
                .reason_codes
                .contains(&"qdrant_hybrid_unavailable".to_owned()),
            "absent source must produce qdrant_hybrid_unavailable reason code; got: {:?}",
            outcome.reason_codes
        );
    }
}
