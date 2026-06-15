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
        ScopedSearchFailure, ScopedSearchResult, merge_scope_results_max,
        search_scopes_concurrently, search_scopes_with_qdrant_candidates,
    },
    fusion::{ScopeRanking, weighted_reciprocal_rank_fusion},
    hybrid::{HybridCandidateSource, HybridQueryError},
    priming_rank::{PrimingRankConfig, select_priming_prime},
    query_segments,
    scope_resolution::DualScopeResolver,
    scoring::ScoringWeights,
};

#[derive(Debug, Clone)]
pub struct SeededSkill {
    pub skill: Skill,
    pub scope_id: String,
    pub source_paths: Vec<PathBuf>,
    /// `e_summary` embedding: `name + description + tags`. The only dense view used
    /// before T09; kept as the primary dense view and the default α term.
    pub embedding: Vec<f32>,
    pub subunits: Vec<Subunit>,
    /// One embedding per entry in `subunits`, in the same order. Used to score the
    /// β (`subunit_evidence`) term of eq.3 by cosine against the query embedding at
    /// request time (issue #172). Empty when a skill has no subunits.
    pub subunit_embeddings: Vec<Vec<f32>>,
    pub prior: f32,
    pub community_boost: f32,
    // ── T09 dense multi-view embeddings ─────────────────────────────────────
    // Built unconditionally at snapshot construction time (always present when the
    // corpus is non-empty) so the `RETRIEVAL_DENSE_VIEWS` flag can be flipped at
    // request time without a graph rebuild. Empty vecs on pre-T03 skills (NULL
    // multi-view DB columns → empty field lists → short/empty view text → embedded
    // to a near-zero-information vector; treated the same as absent by the fusion).
    //
    // DO NOT include `e_negative_embedding` in any positive-fusion path. It carries
    // anti-pattern signal and must never boost a skill for queries that describe
    // situations where the skill must not apply. The fusion reads only `embedding`
    // (e_summary), `e_task_embedding`, and `e_needs_embedding`.
    /// `e_task` embedding: `use_when + subunit_headings + artifacts + tools`.
    /// When empty, the fusion falls back to `e_summary` (zero uplift).
    pub e_task_embedding: Vec<f32>,
    /// `e_needs` embedding: `requires + invariants`.
    /// When empty, the fusion falls back to `e_summary` (zero uplift).
    pub e_needs_embedding: Vec<f32>,
    /// `e_negative` embedding: `avoid_when`.
    /// Built for observability; NEVER used in the positive α fusion.
    /// Deferred scoring use (e.g. penalise skills whose avoid_when matches the
    /// query) is tracked under a future ticket.
    pub e_negative_embedding: Vec<f32>,
}

/// Observability metadata for T09 dense multi-view embeddings.
///
/// Attached to `RetrievalSnapshot` at build time so the health endpoint can report
/// which views were built and their dimensionality — without adding new DB columns.
/// The `view_count` is the number of skills for which ALL three views were built
/// (skills with entirely empty multi-view fields contribute empty embeddings but
/// are still counted, as the text→embedding path ran successfully).
#[derive(Debug, Clone, Default)]
pub struct DenseViewsMetadata {
    /// Names of the dense views built at this snapshot (e.g. `["e_task", "e_needs", "e_negative"]`).
    pub view_names: Vec<String>,
    /// Embedding dimensionality for these views (should equal the model's output dim).
    pub embedding_dim: usize,
    /// Number of skills for which all views were embedded (== skills.len() when non-empty).
    pub skill_count_with_views: usize,
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

/// Boolean flag that parses `0/false/off` → `false` and `1/true/on` → `true`.
///
/// Used by `env_or` for all `RETRIEVAL_*` boolean flags so they accept the same
/// canonical forms as other boolean env-vars in the project. Lowercase and
/// uppercase are both accepted. Anything else fails loud via the `env_or` panic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BoolFlag(pub bool);

impl std::str::FromStr for BoolFlag {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "on" | "yes" | "enabled" => Ok(Self(true)),
            "0" | "false" | "off" | "no" | "disabled" => Ok(Self(false)),
            other => Err(format!(
                "invalid boolean flag {other:?} (expected 1/true/on/yes or 0/false/off/no)"
            )),
        }
    }
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

/// Labels the purpose of a retrieval call so the orchestrator can apply
/// intent-appropriate candidate selection and ranking.
///
/// This type is a typed seam introduced in T12 Unit 1. In this unit `Priming`
/// runs the identical code path as `Task`; later T12 units branch on intent to
/// apply Priming-specific behavior (e.g. broader candidate pool, relaxed floor).
///
/// Threaded via the `SkillRetriever::retrieve` signature so callers declare
/// intent once at the call site; the orchestrator owns the dispatch decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RetrievalIntent {
    /// Mid-session task retrieval (`compile_context` / `find_skill`). Current
    /// behavior; the floor, ranker, and single-view embed are all unchanged.
    #[default]
    Task,
    /// SessionStart priming. Same behavior as `Task` in this unit (pure seam);
    /// Priming-specific behavior is added in later T12 units.
    Priming,
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
    /// T09 dense multi-view observability metadata. Populated by `build_graph_from_pg`
    /// at snapshot construction time. Empty (default) for cold-start snapshots and
    /// test snapshots that do not build the views. Consulted by the health endpoint to
    /// surface view names, embedding dim, and skill count without new DB columns.
    pub dense_views_metadata: DenseViewsMetadata,
    /// T12 Unit 3: Stable skill id → whole days since `skills.created_at`.
    ///
    /// Used by `select_priming_prime` to identify "fresh" skills (age ≤
    /// `priming_freshness_window_days`) eligible for freshness slot injection.
    ///
    /// Populated by `build_graph_from_pg` at snapshot construction time. Default
    /// **empty** from `RetrievalSnapshot::new` so that cold-start snapshots and
    /// test snapshots carry no freshness data — zero behavior change for every
    /// existing test. An unknown skill (not in this map) is never treated as fresh.
    ///
    /// NOTE: the corpus was rebuilt in one go so wall-clock `created_at` may be
    /// near-uniform (all skills appear "fresh" together). This is an honest measured
    /// outcome; the Unit 4 measurement will quantify the impact.
    pub skill_age_days: HashMap<String, u32>,
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
            dense_views_metadata: DenseViewsMetadata::default(),
            skill_age_days: HashMap::new(),
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

    /// Attaches T09 dense multi-view observability metadata (builder style).
    ///
    /// Called by `build_graph_from_pg` after the multi-view embeddings are built
    /// so the snapshot carries the metadata from first use. Tests that do not build
    /// the dense views can skip this call; `dense_views_metadata` defaults to empty.
    pub fn with_dense_views_metadata(mut self, metadata: DenseViewsMetadata) -> Self {
        self.dense_views_metadata = metadata;
        self
    }

    /// Attaches T12 Unit 3 skill age data (builder style).
    ///
    /// Maps each skill's stable id to whole days since `skills.created_at`.
    /// Called by `build_graph_from_pg` after the skills are loaded so the snapshot
    /// carries freshness data from its first use. Tests that do not need freshness
    /// injection can skip this call; `skill_age_days` defaults to empty (no freshness
    /// data → no skill is considered fresh → zero behavior change for existing tests).
    pub fn with_skill_age_days(mut self, age_map: HashMap<String, u32>) -> Self {
        self.skill_age_days = age_map;
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
    /// When `true`, the α (`l1_semantic`) term fuses three dense views —
    /// {`e_summary`, `e_task`, `e_needs`} — by taking the max cosine, instead of
    /// using only `e_summary`. `e_negative` is built and observable but NEVER
    /// enters the positive fusion regardless of this flag.
    ///
    /// **Default: `true`** (on) since the T11 sweep (2026-06-11) measured a
    /// validated uplift on the rich 262-skill corpus. Disable via
    /// `RETRIEVAL_DENSE_VIEWS=false/0/off` to restore pre-T09 single-view ranking.
    /// A present-but-unparseable value panics fail-loud (no silent fallback).
    ///
    /// With the flag off the α term equals `cosine(prompt, e_summary)` exactly —
    /// byte-for-byte identical to the pre-T09 behaviour. The view embeddings are
    /// always built at snapshot construction time (so a restart with the flag
    /// toggled needs no graph rebuild), but they are only READ by the scoring path
    /// when this flag is on.
    pub dense_views_enabled: bool,

    // ── T12 Unit 3: Priming-scoped retrieval parameters ─────────────────────
    // These fields are ONLY applied when `RetrievalIntent::Priming` is active.
    // The `Task` path and the global `relevance_threshold` (0.48) are UNTOUCHED.
    // Defaults are deliberately conservative so Unit 4 can measure each lever
    // independently on the real server without silent behavior changes at Task sites.
    /// Relevance floor applied to the Priming candidate pool.
    ///
    /// Lower than the Task floor (0.48) to surface more of the broad baseline gold
    /// set while still discriminating — the negative-control permutation gate must
    /// still crater. Default **0.30** (env `RETRIEVAL_PRIMING_RELEVANCE_THRESHOLD`).
    pub priming_relevance_threshold: f32,

    /// Maximum number of candidates to include in the SessionStart prime.
    ///
    /// Bounded prime size keeps the context injection tight. Default **5**
    /// (env `RETRIEVAL_PRIMING_MAX_RESULTS`).
    pub priming_max_results: usize,

    /// Additive weight for the recurrence (usage prior) signal in the Priming rerank.
    ///
    /// Applied as `score + recurrence_weight * prior` where prior ≤ 0.15, so
    /// the maximum boost is ≈ 0.015 with the default weight — relevance stays
    /// dominant. Default **0.10** (env `RETRIEVAL_PRIMING_RECURRENCE_WEIGHT`).
    pub priming_recurrence_weight: f32,

    /// Number of bottom slots reserved for freshness injection in the Priming prime.
    ///
    /// A fresh skill ranked just outside the top-N may displace the lowest non-fresh
    /// slot if a freshness slot is available. Default **1**
    /// (env `RETRIEVAL_PRIMING_FRESHNESS_SLOTS`).
    pub priming_freshness_slots: usize,

    /// Age threshold in days for a skill to be considered "fresh" for slot injection.
    ///
    /// A skill whose `age_days ≤ priming_freshness_window_days` is eligible for a
    /// freshness slot. Unknown age (missing from the snapshot map) → never fresh.
    /// Default **30** (env `RETRIEVAL_PRIMING_FRESHNESS_WINDOW_DAYS`).
    pub priming_freshness_window_days: u32,

    /// Maximum number of query segments embedded for the Priming query-side
    /// multi-view (T12 Unit 2). Each segment is a separate embed; the Ollama
    /// semaphore serializes them, so this is the dominant SessionStart latency
    /// lever for long/verbose openings. Lower = faster but fewer query views.
    /// Default **8** (env `RETRIEVAL_PRIMING_MAX_SEGMENTS`). A value of 1 reduces
    /// Priming to a single full-prompt embed (no query-side multi-view).
    pub priming_max_segments: usize,
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
            // T09: multi-view dense fusion — DEFAULT-ON since the T11 sweep
            // (2026-06-11) measured a validated uplift on the rich 262-skill qwen3
            // corpus: anchor MRR@3 0.686→0.743, candidate-recall@50 0.723→0.796,
            // nDCG@3 0.696→0.755 (sign p=0.0074); judge-aug held-out 0.912/0.839/0.92
            // vs dense 0.884/0.804/0.92, p95 369ms < 500ms SLO. The view embeddings
            // are built unconditionally; this flag gates only the scoring read.
            // Set RETRIEVAL_DENSE_VIEWS=false to restore byte-for-byte pre-T09 ranking.
            // See tests/e2e/reports/t11/T11-VALIDATION-REPORT.md.
            dense_views_enabled: true,

            // T12 Unit 3: Priming-scoped parameters (conservative defaults).
            // PRIMING-ONLY: the Task path and global floor (0.48) are UNTOUCHED.
            //
            // priming_relevance_threshold: lower than 0.48 so more of the baseline
            // gold set surfaces, while still discriminating (permutation control must crater).
            priming_relevance_threshold: 0.30,
            // priming_max_results: bounded prime size (session context is precious).
            priming_max_results: 5,
            // priming_recurrence_weight: modest usage-prior additive boost.
            // Max boost = 0.10 * 0.15 (max prior) = 0.015 — relevance stays dominant.
            priming_recurrence_weight: 0.10,
            // priming_freshness_slots: one reserved slot for the most-relevant fresh skill.
            priming_freshness_slots: 1,
            // priming_freshness_window_days: skills ≤30 days old are "fresh".
            priming_freshness_window_days: 30,
            // priming_max_segments: query-side multi-view cap (latency lever).
            priming_max_segments: query_segments::DEFAULT_MAX_SEGMENTS,
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
    /// `RETRIEVAL_BACKEND` (`snapshot_dense`|`snapshot_hybrid`|`qdrant_hybrid`),
    /// `RETRIEVAL_DENSE_VIEWS` (`0/false/off` or `1/true/on`, default `true` since T11),
    /// `RETRIEVAL_PRIMING_RELEVANCE_THRESHOLD` (f32, default 0.30),
    /// `RETRIEVAL_PRIMING_MAX_RESULTS` (usize, default 5),
    /// `RETRIEVAL_PRIMING_RECURRENCE_WEIGHT` (f32, default 0.10),
    /// `RETRIEVAL_PRIMING_FRESHNESS_SLOTS` (usize, default 1),
    /// `RETRIEVAL_PRIMING_FRESHNESS_WINDOW_DAYS` (u32, default 30).
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
            dense_views_enabled: env_or("RETRIEVAL_DENSE_VIEWS", BoolFlag(d.dense_views_enabled)).0,
            // T12 Unit 3: Priming-scoped overrides (fail-loud; absent → documented default).
            priming_relevance_threshold: env_or(
                "RETRIEVAL_PRIMING_RELEVANCE_THRESHOLD",
                d.priming_relevance_threshold,
            ),
            priming_max_results: env_or("RETRIEVAL_PRIMING_MAX_RESULTS", d.priming_max_results),
            priming_recurrence_weight: env_or(
                "RETRIEVAL_PRIMING_RECURRENCE_WEIGHT",
                d.priming_recurrence_weight,
            ),
            priming_freshness_slots: env_or(
                "RETRIEVAL_PRIMING_FRESHNESS_SLOTS",
                d.priming_freshness_slots,
            ),
            priming_freshness_window_days: env_or(
                "RETRIEVAL_PRIMING_FRESHNESS_WINDOW_DAYS",
                d.priming_freshness_window_days,
            ),
            priming_max_segments: env_or("RETRIEVAL_PRIMING_MAX_SEGMENTS", d.priming_max_segments),
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
    /// Retrieves the top matching skills for `prompt` in the given repository scope.
    ///
    /// `intent` classifies the caller's purpose (`Task` for mid-session queries,
    /// `Priming` for SessionStart pre-loading). In T12 Unit 1 both intents run the
    /// same code path; later T12 units branch on intent for Priming-specific behavior.
    async fn retrieve(
        &self,
        prompt: &str,
        repo_path: Option<&str>,
        intent: RetrievalIntent,
    ) -> RetrievalOutcome;
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
    async fn retrieve(
        &self,
        prompt: &str,
        repo_path: Option<&str>,
        intent: RetrievalIntent,
    ) -> RetrievalOutcome {
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

        // Gate the embedding call(s) behind the circuit breaker.  When the breaker
        // is open the provider has been repeatedly failing; skip the network call
        // entirely and return a loud degraded outcome with a named reason code so
        // callers can distinguish circuit-open from a transient embed failure.
        //
        // Manual allow/record (not `execute_with_resilience`) because
        // OllamaEmbeddingService already carries internal timeouts and the embed
        // batch path uses `retry_with_backoff`; stacking a second retry layer
        // here would violate the project's no-double-retry rule.
        if !self.embedding_breaker.allow_request().await {
            reason_codes.push(Self::REASON_EMBEDDING_CIRCUIT_OPEN.to_owned());
            return self.build_degraded_outcome(
                started,
                graph_version,
                scopes_considered.clone(),
                reason_codes,
                scopes_considered,
            );
        }

        // Dispatch to the configured candidate-generation backend, applying
        // intent-aware embedding and candidate-generation strategy:
        //
        // - `QdrantHybrid` (any intent): always single-embed; Qdrant is the
        //   candidate source; segmentation is NOT applied (T12 Unit 2 scope).
        // - `SnapshotDense`/`SnapshotHybrid` with `Task`: single embed, single
        //   search pass — byte-identical to pre-T12 behavior.
        // - `SnapshotDense`/`SnapshotHybrid` with `Priming`: segment the prompt,
        //   embed all segments in ONE batch call (latency fence), run one search
        //   pass per segment, and merge by max-score per (scope_id, skill_id).
        //   With a 1-segment prompt (short / Task-like opening) the merge is a
        //   no-op and the result is numerically identical to the Task path.
        let (scope_results, scope_failures) = match self.config.backend {
            RetrievalBackend::QdrantHybrid => {
                // QdrantHybrid: always single-embed regardless of intent.
                let prompt_embedding = match self.embedding_service.embed_text(prompt).await {
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
                };
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

            RetrievalBackend::SnapshotDense | RetrievalBackend::SnapshotHybrid => {
                match intent {
                    RetrievalIntent::Task => {
                        // Task: single embed, single search pass — unchanged from pre-T12.
                        let prompt_embedding = match self.embedding_service.embed_text(prompt).await
                        {
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
                        };
                        search_scopes_concurrently(
                            prompt,
                            &prompt_embedding,
                            graph.clone(),
                            &self.config,
                            &scopes,
                        )
                        .await
                    }

                    RetrievalIntent::Priming => {
                        // T12 Unit 3: Build a Priming-scoped effective config that
                        // overrides the relevance floor and max_results for the search
                        // passes so `score_and_select_candidates` applies the lower
                        // Priming floor. The Task path is UNTOUCHED (uses `&self.config`).
                        //
                        // Only `relevance_threshold` and `max_results` differ; all other
                        // fields (scoring weights, rrf_k, candidate_limit, scope weights,
                        // dense_views_enabled, etc.) are inherited from `self.config` so
                        // the ranking logic is identical — only the floor changes.
                        let priming_search_config = RetrievalConfig {
                            relevance_threshold: self.config.priming_relevance_threshold,
                            max_results: self.config.priming_max_results,
                            ..self.config.clone()
                        };

                        // Segment the prompt into topically-distinct views (pure string
                        // work, no LLM). For a short/single-paragraph prompt this yields
                        // exactly 1 segment and the path is numerically identical to Task
                        // (when floors also match).
                        let segments = query_segments::segment_prompt(
                            prompt,
                            self.config.priming_max_segments,
                            query_segments::DEFAULT_MAX_SEGMENT_CHARS,
                        );

                        // ONE batched embedding call for all segments (latency fence).
                        // Even a 1-segment batch costs one network round-trip — same as
                        // the Task `embed_text` call — preserving the latency budget.
                        let segment_refs: Vec<&str> = segments.iter().map(String::as_str).collect();
                        let segment_embeddings =
                            match self.embedding_service.embed_batch(&segment_refs).await {
                                Ok(embeddings) => {
                                    self.embedding_breaker.record_success().await;
                                    embeddings
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
                            };

                        // One search pass per segment embedding, using the Priming
                        // effective config (lower floor) for each pass.
                        // Arc::clone per pass is a cheap refcount bump.
                        let mut passes: Vec<(Vec<ScopedSearchResult>, Vec<ScopedSearchFailure>)> =
                            Vec::with_capacity(segment_embeddings.len());
                        for seg_embedding in &segment_embeddings {
                            let pass = search_scopes_concurrently(
                                prompt,
                                seg_embedding,
                                graph.clone(),
                                &priming_search_config,
                                &scopes,
                            )
                            .await;
                            passes.push(pass);
                        }

                        // Merge by max score per (scope_id, skill_id) across passes.
                        // A 1-segment batch produces exactly 1 pass → merge is a no-op.
                        merge_scope_results_max(passes, self.config.candidate_limit)
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

        // T12 Unit 3: choose the fusion bound and candidate selection strategy by intent.
        //
        // - Task: unchanged — plain top-`max_results` by RRF score.
        // - Priming: uses `priming_max_results` as the bound, then delegates final
        //   ordering/injection to `select_priming_prime` (recurrence + freshness).
        //
        // The `fusion_limit` upper-bounds how many entries RRF returns before
        // selection. For Task it is ≥ `max_results`; for Priming ≥ `priming_max_results`.
        // We take the larger of all-candidates-sum and the effective N so that
        // `select_priming_prime` has the full pool to draw fresh candidates from.
        let effective_max = match intent {
            RetrievalIntent::Task => self.config.max_results,
            RetrievalIntent::Priming => self.config.priming_max_results,
        };

        let fusion_limit = scope_rankings
            .iter()
            .map(|ranking| ranking.candidates.len())
            .sum::<usize>()
            .max(effective_max);

        let ranked_candidates =
            weighted_reciprocal_rank_fusion(&scope_rankings, self.config.rrf_k, fusion_limit);

        let selected_candidates: Vec<_> = match intent {
            RetrievalIntent::Task => {
                // Byte-identical to pre-T12: plain top-N by RRF score.
                ranked_candidates
                    .iter()
                    .take(self.config.max_results)
                    .cloned()
                    .collect()
            }
            RetrievalIntent::Priming => {
                // Priming: recurrence rerank + bounded freshness injection.
                let priming_cfg = PrimingRankConfig {
                    max_results: self.config.priming_max_results,
                    recurrence_weight: self.config.priming_recurrence_weight,
                    freshness_slots: self.config.priming_freshness_slots,
                    freshness_window_days: self.config.priming_freshness_window_days,
                };
                let prior_of = |idx: usize| -> f32 {
                    ranked_candidates
                        .get(idx)
                        .and_then(|c| graph.skills.get(c.skill_index))
                        .map(|seeded| seeded.prior)
                        .unwrap_or(0.0)
                };
                let age_days_of =
                    |skill_id: &str| -> Option<u32> { graph.skill_age_days.get(skill_id).copied() };
                let selected_indices =
                    select_priming_prime(&ranked_candidates, prior_of, age_days_of, priming_cfg);
                selected_indices
                    .into_iter()
                    .filter_map(|idx| ranked_candidates.get(idx).cloned())
                    .collect()
            }
        };
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
                        semantic_score: candidate.semantic_score,
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

        let mut health = if degraded_scopes.is_empty() {
            Self::healthy_markers_for_backend(self.config.backend)
        } else {
            let reason = reason_codes
                .first()
                .cloned()
                .unwrap_or_else(|| "retrieval_degraded".to_owned());
            Self::degraded_marker_for_backend(self.config.backend, &reason)
        };

        // T09: surface dense multi-view observability in the per-request health
        // marker so the orchestrator's live sweep can confirm views were built
        // without adding new DB columns. Only emitted when views were actually
        // built (non-empty metadata); test snapshots and cold-start snapshots
        // with no skills produce empty metadata and no markers are added.
        let dvm = &graph.dense_views_metadata;
        if !dvm.view_names.is_empty() {
            health.insert("dense_views_built".to_owned(), dvm.view_names.join(","));
            health.insert("dense_views_dim".to_owned(), dvm.embedding_dim.to_string());
            health.insert(
                "dense_views_skill_count".to_owned(),
                dvm.skill_count_with_views.to_string(),
            );
        }

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
                // T09 view embeddings empty in test fixtures (not needed for
                // the concurrency probe; dense_views_enabled stays false).
                e_task_embedding: Vec::new(),
                e_needs_embedding: Vec::new(),
                e_negative_embedding: Vec::new(),
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

                    let outcome = orchestrator
                        .retrieve("probe", None, RetrievalIntent::Task)
                        .await;
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
            let outcome = orchestrator
                .retrieve("probe", None, RetrievalIntent::Task)
                .await;
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
        let open_outcome = orchestrator
            .retrieve("probe-after-open", None, RetrievalIntent::Task)
            .await;
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
        let probe_outcome = orchestrator
            .retrieve("probe", None, RetrievalIntent::Task)
            .await;
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
                    e_task_embedding: Vec::new(),
                    e_needs_embedding: Vec::new(),
                    e_negative_embedding: Vec::new(),
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
                    e_task_embedding: Vec::new(),
                    e_needs_embedding: Vec::new(),
                    e_negative_embedding: Vec::new(),
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

        let outcome = orchestrator
            .retrieve("query alpha", None, RetrievalIntent::Task)
            .await;

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

        let outcome = orchestrator
            .retrieve("query alpha", None, RetrievalIntent::Task)
            .await;

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

        let outcome = orchestrator
            .retrieve("query", None, RetrievalIntent::Task)
            .await;

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

    // ── T09: RETRIEVAL_DENSE_VIEWS flag tests ───────────────────────────────

    /// T09/T11 guard: `RETRIEVAL_DENSE_VIEWS` defaults to TRUE since the T11 sweep
    /// (2026-06-11) measured a validated uplift on the rich 262-skill corpus.
    /// Multi-view dense fusion is the default; `RETRIEVAL_DENSE_VIEWS=false` opts back
    /// out to the pre-T09 single-view ranking (covered by
    /// `dense_views_disabled_when_env_is_false`).
    #[test]
    fn dense_views_default_is_true_after_t11_validation() {
        let _guard = EnvVarGuard::remove("RETRIEVAL_DENSE_VIEWS");
        let config = RetrievalConfig::from_env();
        assert!(
            config.dense_views_enabled,
            "RETRIEVAL_DENSE_VIEWS must default to true after the T11 validation; got false"
        );
        // Also check the typed default directly.
        assert!(
            RetrievalConfig::default().dense_views_enabled,
            "RetrievalConfig::default().dense_views_enabled must be true"
        );
    }

    /// T09 flag parsing: canonical true-like values.
    #[test]
    fn dense_views_flag_parses_true_like_values() {
        use std::str::FromStr;
        for val in &["1", "true", "on", "yes", "enabled", "TRUE", "ON"] {
            assert_eq!(
                BoolFlag::from_str(val).unwrap(),
                BoolFlag(true),
                "expected BoolFlag(true) for {:?}",
                val
            );
        }
    }

    /// T09 flag parsing: canonical false-like values.
    #[test]
    fn dense_views_flag_parses_false_like_values() {
        use std::str::FromStr;
        for val in &["0", "false", "off", "no", "disabled", "FALSE", "OFF"] {
            assert_eq!(
                BoolFlag::from_str(val).unwrap(),
                BoolFlag(false),
                "expected BoolFlag(false) for {:?}",
                val
            );
        }
    }

    /// T09 flag parsing: rejects unknown values fail-loud.
    #[test]
    fn dense_views_flag_rejects_unknown_values() {
        use std::str::FromStr;
        for val in &["2", "maybe", "y", "n", "enabled-but-maybe"] {
            assert!(
                BoolFlag::from_str(val).is_err(),
                "expected Err for unknown bool value {:?}",
                val
            );
        }
    }

    /// T09 guard: with RETRIEVAL_DENSE_VIEWS=true the config sets dense_views_enabled.
    #[test]
    fn dense_views_enabled_when_env_is_true() {
        let _guard = EnvVarGuard::set("RETRIEVAL_DENSE_VIEWS", "true");
        let config = RetrievalConfig::from_env();
        assert!(
            config.dense_views_enabled,
            "RETRIEVAL_DENSE_VIEWS=true must set dense_views_enabled=true"
        );
    }

    /// T09 guard: with RETRIEVAL_DENSE_VIEWS=1 the config sets dense_views_enabled.
    #[test]
    fn dense_views_enabled_when_env_is_one() {
        let _guard = EnvVarGuard::set("RETRIEVAL_DENSE_VIEWS", "1");
        let config = RetrievalConfig::from_env();
        assert!(
            config.dense_views_enabled,
            "RETRIEVAL_DENSE_VIEWS=1 must set dense_views_enabled=true"
        );
    }

    /// T09 guard: with RETRIEVAL_DENSE_VIEWS=false the config disables dense_views.
    #[test]
    fn dense_views_disabled_when_env_is_false() {
        let _guard = EnvVarGuard::set("RETRIEVAL_DENSE_VIEWS", "false");
        let config = RetrievalConfig::from_env();
        assert!(
            !config.dense_views_enabled,
            "RETRIEVAL_DENSE_VIEWS=false must set dense_views_enabled=false"
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

        let outcome = orchestrator
            .retrieve("query", None, RetrievalIntent::Task)
            .await;

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

    // ── T12 Unit 1: RetrievalIntent seam tests ──────────────────────────────

    /// T12 Unit 1 guard: `RetrievalIntent::default()` is `Task` — the existing
    /// call sites remain on the Task path with no behavior change.
    #[test]
    fn retrieval_intent_default_is_task() {
        assert_eq!(
            RetrievalIntent::default(),
            RetrievalIntent::Task,
            "default intent must be Task so all existing call sites retain current behavior"
        );
    }

    // ── T12 Unit 2: keyword-aware embedding service for behavioral tests ──────

    /// Test-only embedding service that returns different vectors based on whether
    /// the input text contains a known keyword. Used to prove that query-side
    /// multi-view (max-over-segments) surfaces skills that match a segment of a
    /// verbose prompt even when the averaged / whole-prompt embedding does not.
    ///
    /// Keyword → 4D unit vector mapping:
    /// - "auth" keyword in text → `[1.0, 0.0, 0.0, 0.0]`  (dimension 0 = auth topic)
    /// - "migration" keyword    → `[0.0, 1.0, 0.0, 0.0]`  (dimension 1 = migration topic)
    /// - anything else          → `[0.5, 0.5, 0.0, 0.0]`  (blended / off-topic)
    struct KeywordAwareEmbeddingService;

    #[async_trait::async_trait]
    impl EmbeddingService for KeywordAwareEmbeddingService {
        async fn embed_text(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
            Ok(keyword_vector(text))
        }

        async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
            Ok(texts.iter().map(|t| keyword_vector(t)).collect())
        }
    }

    fn keyword_vector(text: &str) -> Vec<f32> {
        let lower = text.to_lowercase();
        if lower.contains("auth") {
            vec![1.0, 0.0, 0.0, 0.0]
        } else if lower.contains("migration") {
            vec![0.0, 1.0, 0.0, 0.0]
        } else {
            vec![0.5, 0.5, 0.0, 0.0]
        }
    }

    /// Builds a two-skill snapshot for the Priming multi-view behavioral test.
    ///
    /// - `auth-skill`:      embedding `[1.0, 0.0, 0.0, 0.0]` — matches "auth" queries.
    /// - `migration-skill`: embedding `[0.0, 1.0, 0.0, 0.0]` — matches "migration" queries.
    fn keyword_snapshot() -> RetrievalSnapshot {
        use domain::{DomainId, LifecycleStatus, ScopeType, Skill, SkillStatus};

        let auth_skill = Skill {
            id: DomainId::new_unchecked("auth-skill"),
            name: "authentication middleware".to_owned(),
            description: "auth token middleware".to_owned(),
            scope: ScopeType::Global,
            status: SkillStatus::Ready,
            lifecycle: LifecycleStatus::Active,
            tags: vec!["auth".to_owned()],
            subunit_ids: vec![],
            community_id: None,
        };
        let migration_skill = Skill {
            id: DomainId::new_unchecked("migration-skill"),
            name: "database migration".to_owned(),
            description: "schema migration tooling".to_owned(),
            scope: ScopeType::Global,
            status: SkillStatus::Ready,
            lifecycle: LifecycleStatus::Active,
            tags: vec!["migration".to_owned()],
            subunit_ids: vec![],
            community_id: None,
        };

        RetrievalSnapshot::new(
            vec![
                SeededSkill {
                    skill: auth_skill,
                    scope_id: "global".to_owned(),
                    source_paths: vec![],
                    embedding: vec![1.0, 0.0, 0.0, 0.0],
                    subunits: vec![],
                    subunit_embeddings: vec![],
                    prior: 0.0,
                    community_boost: 0.0,
                    e_task_embedding: Vec::new(),
                    e_needs_embedding: Vec::new(),
                    e_negative_embedding: Vec::new(),
                },
                SeededSkill {
                    skill: migration_skill,
                    scope_id: "global".to_owned(),
                    source_paths: vec![],
                    embedding: vec![0.0, 1.0, 0.0, 0.0],
                    subunits: vec![],
                    subunit_embeddings: vec![],
                    prior: 0.0,
                    community_boost: 0.0,
                    e_task_embedding: Vec::new(),
                    e_needs_embedding: Vec::new(),
                    e_negative_embedding: Vec::new(),
                },
            ],
            1,
        )
    }

    /// T12 Unit 2 / Unit 3 single-segment no-divergence guard:
    /// Priming with a short single-segment prompt and a config where BOTH the Task
    /// floor and the Priming floor are set equal (0.1 test floor) produces the same
    /// result as Task (byte-identical for 1 segment when floors match).
    ///
    /// Uses `ConstantEmbeddingService` + the explicit keyword_snapshot so both arms
    /// find the same skills at the same scores. The invariant is: with 1 segment,
    /// the Priming merge is a no-op; with equal floors the only remaining difference
    /// is max_results (set equal here too). This proves the single-segment path is
    /// preserved even after Unit 3's floor branching.
    ///
    /// NOTE: the default config diverges Task/Priming on `versioned_snapshot(3)` because
    /// eq3=0.45 < Task floor 0.48 but ≥ Priming floor 0.30. The divergence is correct
    /// and is the whole point of Unit 3 (see `priming_lower_floor_surfaces_skill_below_task_threshold`).
    /// This guard uses matching floors to prove the segment merge path has no bugs.
    #[tokio::test]
    async fn priming_single_segment_prompt_equals_task_outcome() {
        // Equal floors + equal max_results → single-segment Priming == Task.
        let config = RetrievalConfig {
            scope_id: "global".to_owned(),
            scope_type: domain::ScopeType::Global,
            candidate_limit: 10,
            max_results: 3,
            relevance_threshold: 0.1,         // Task floor
            priming_relevance_threshold: 0.1, // same Priming floor — no divergence
            priming_max_results: 3,           // same max_results — no divergence
            dense_views_enabled: false,
            backend: RetrievalBackend::SnapshotDense,
            ..RetrievalConfig::default()
        };

        let orchestrator = RetrievalOrchestrator::new(
            Arc::new(KeywordAwareEmbeddingService),
            keyword_snapshot(),
            config,
        );

        // Short single-word prompt → 1 segment → merge is a no-op.
        let task_outcome = orchestrator
            .retrieve("auth", None, RetrievalIntent::Task)
            .await;
        let priming_outcome = orchestrator
            .retrieve("auth", None, RetrievalIntent::Priming)
            .await;

        let task_ids: Vec<_> = task_outcome
            .skills
            .iter()
            .map(|s| {
                (
                    s.scored_skill.skill.id.as_str().to_owned(),
                    s.scored_skill.score.to_bits(),
                )
            })
            .collect();
        let priming_ids: Vec<_> = priming_outcome
            .skills
            .iter()
            .map(|s| {
                (
                    s.scored_skill.skill.id.as_str().to_owned(),
                    s.scored_skill.score.to_bits(),
                )
            })
            .collect();
        assert_eq!(
            task_ids, priming_ids,
            "T12 Unit 2/3: single-segment Priming with equal floors must equal Task (byte-identical single-view merge)"
        );
        assert_eq!(
            task_outcome.reason_codes, priming_outcome.reason_codes,
            "T12 Unit 2/3: Priming reason_codes must equal Task for short prompts with equal floors"
        );
    }

    /// T12 Unit 2 behavioral test: Priming with a multi-paragraph verbose prompt
    /// surfaces `migration-skill` (which matches only segment 2) even though the
    /// full-prompt averaged embedding (`[0.5, 0.5]` from KeywordAwareEmbeddingService)
    /// does not produce a cosine-1.0 hit for either skill.
    ///
    /// Under Task (single embed = `[0.5, 0.5, 0.0, 0.0]`):
    ///   cosine(`[0.5,0.5]`, auth-skill `[1.0,0.0]`) = 0.707 → eq3 ≈ 0.318 (below 0.48 floor)
    ///   cosine(`[0.5,0.5]`, migration `[0.0,1.0]`) = 0.707 → same (below floor)
    ///   Both skills are gated out → empty result.
    ///
    /// Under Priming (2 segments: "auth middleware", "database migration"):
    ///   Segment 1 embed = `[1.0,0,0,0]` → cosine(auth-skill) = 1.0 → eq3 = 0.45 (below floor)
    ///   Segment 2 embed = `[0,1.0,0,0]` → cosine(migration-skill) = 1.0 → eq3 = 0.45 (below floor)
    ///
    /// NOTE: with the default floor 0.48 and no subunit evidence (β=0) and no prior (γ=0),
    /// even a perfect cosine hit gives eq3 = 0.45 which is still below the floor.
    /// So we use a low test floor (0.1) to prove the behavioral difference.
    #[tokio::test]
    async fn priming_multi_segment_surfaces_skill_matching_only_second_paragraph() {
        // Low floor so cosine hits above 0 clear it.
        let config = RetrievalConfig {
            scope_id: "global".to_owned(),
            scope_type: domain::ScopeType::Global,
            candidate_limit: 10,
            max_results: 3,
            relevance_threshold: 0.1, // low floor for this behavioral test
            backend: RetrievalBackend::SnapshotDense,
            dense_views_enabled: false, // single e_summary view — keep it simple
            ..RetrievalConfig::default()
        };

        let orchestrator = RetrievalOrchestrator::new(
            Arc::new(KeywordAwareEmbeddingService),
            keyword_snapshot(),
            config,
        );

        // Two-paragraph prompt: para 1 = auth topic, para 2 = migration topic.
        // The `segment_prompt` function splits on "\n\n" → 2 segments.
        let verbose_prompt = "Implement auth middleware for the request pipeline.\n\nDatabase migration tooling for schema evolution.";

        // Task: embed the whole prompt as one vector.
        // KeywordAwareEmbeddingService returns [0.5,0.5,0,0] for text containing both topics.
        let task_outcome = orchestrator
            .retrieve(verbose_prompt, None, RetrievalIntent::Task)
            .await;

        // Priming: embed each paragraph separately.
        // Para 1 → [1.0,0,0,0] (auth) → cosine with auth-skill = 1.0
        // Para 2 → [0.0,1.0,0,0] (migration) → cosine with migration-skill = 1.0
        let priming_outcome = orchestrator
            .retrieve(verbose_prompt, None, RetrievalIntent::Priming)
            .await;

        // Task uses [0.5,0.5,0,0]: cosine with auth=[1,0,0,0] = 0.5/sqrt(0.5^2+0.5^2) = 0.707
        // But note: KeywordAwareEmbeddingService returns [0.5,0.5,0,0] for mixed-topic text,
        // and cosine([0.5,0.5,0,0], [1,0,0,0]) = 0.5 / sqrt(0.5) = 0.707... eq3 = 0.45*0.707 = 0.318
        // With floor=0.1 → Task DOES find both skills but at lower scores.
        // Priming segments the prompt → para1 = "auth" keyword → [1,0,0,0] → cosine(auth-skill)=1.0
        //                              → para2 = "migration" → [0,1,0,0] → cosine(migration-skill)=1.0
        // So Priming finds BOTH skills at score 0.45 (α * 1.0), vs Task finding at 0.318.
        // The test verifies Priming surfaces migration-skill (matching segment 2 only).
        let priming_skill_ids: Vec<&str> = priming_outcome
            .skills
            .iter()
            .map(|s| s.scored_skill.skill.id.as_str())
            .collect();

        assert!(
            priming_skill_ids.contains(&"migration-skill"),
            "Priming must surface migration-skill (matches only the second paragraph segment); \
             got skills: {:?}",
            priming_skill_ids
        );
        assert!(
            priming_skill_ids.contains(&"auth-skill"),
            "Priming must surface auth-skill (matches only the first paragraph segment); \
             got skills: {:?}",
            priming_skill_ids
        );

        // Verify Priming scores are at or above Task scores for both skills
        // (max-over-segments = higher quality match per skill vs averaged embedding).
        let task_migration_score = task_outcome
            .skills
            .iter()
            .find(|s| s.scored_skill.skill.id.as_str() == "migration-skill")
            .map(|s| s.scored_skill.score);
        let priming_migration_score = priming_outcome
            .skills
            .iter()
            .find(|s| s.scored_skill.skill.id.as_str() == "migration-skill")
            .map(|s| s.scored_skill.score);

        if let (Some(task_score), Some(priming_score)) =
            (task_migration_score, priming_migration_score)
        {
            assert!(
                priming_score >= task_score,
                "Priming migration-skill score {priming_score:.4} must be >= Task score {task_score:.4} \
                 (segment 2 = pure migration vector; Task = blended vector)"
            );
        }
    }

    // ── T12 Unit 3: priming-scoped config, freshness snapshot, floor divergence ──

    /// T12 Unit 3: `RetrievalConfig::default()` carries the correct defaults for
    /// all five new priming-scoped fields introduced in Unit 3.
    ///
    /// These defaults are deliberately conservative so Unit 4 can measure each lever
    /// independently on the real server: threshold 0.30 (lower floor), max_results 5
    /// (small prime), recurrence_weight 0.10 (modest prior boost), freshness_slots 1
    /// (single injection slot), freshness_window_days 30 (one month = "fresh").
    #[test]
    fn priming_config_defaults_are_conservative() {
        let cfg = RetrievalConfig::default();
        assert!(
            (cfg.priming_relevance_threshold - 0.30).abs() < 1e-6,
            "priming_relevance_threshold default must be 0.30; got {}",
            cfg.priming_relevance_threshold
        );
        assert_eq!(
            cfg.priming_max_results, 5,
            "priming_max_results default must be 5"
        );
        assert!(
            (cfg.priming_recurrence_weight - 0.10).abs() < 1e-6,
            "priming_recurrence_weight default must be 0.10; got {}",
            cfg.priming_recurrence_weight
        );
        assert_eq!(
            cfg.priming_freshness_slots, 1,
            "priming_freshness_slots default must be 1"
        );
        assert_eq!(
            cfg.priming_freshness_window_days, 30,
            "priming_freshness_window_days default must be 30"
        );
        assert_eq!(
            cfg.priming_max_segments,
            query_segments::DEFAULT_MAX_SEGMENTS,
            "priming_max_segments default must be DEFAULT_MAX_SEGMENTS"
        );
    }

    /// T12 Unit 3: `RetrievalConfig::from_env()` parses all five priming-scoped fields
    /// from their dedicated env vars (fail-loud; absent = default).
    #[test]
    fn priming_config_env_parses_all_five_fields() {
        let _g1 = EnvVarGuard::set("RETRIEVAL_PRIMING_RELEVANCE_THRESHOLD", "0.20");
        let _g2 = EnvVarGuard::set("RETRIEVAL_PRIMING_MAX_RESULTS", "7");
        let _g3 = EnvVarGuard::set("RETRIEVAL_PRIMING_RECURRENCE_WEIGHT", "0.05");
        let _g4 = EnvVarGuard::set("RETRIEVAL_PRIMING_FRESHNESS_SLOTS", "2");
        let _g5 = EnvVarGuard::set("RETRIEVAL_PRIMING_FRESHNESS_WINDOW_DAYS", "60");
        let _g6 = EnvVarGuard::set("RETRIEVAL_PRIMING_MAX_SEGMENTS", "3");

        let cfg = RetrievalConfig::from_env();

        assert_eq!(
            cfg.priming_max_segments, 3,
            "RETRIEVAL_PRIMING_MAX_SEGMENTS=3 must parse"
        );
        assert!(
            (cfg.priming_relevance_threshold - 0.20).abs() < 1e-6,
            "RETRIEVAL_PRIMING_RELEVANCE_THRESHOLD=0.20 must parse; got {}",
            cfg.priming_relevance_threshold
        );
        assert_eq!(
            cfg.priming_max_results, 7,
            "RETRIEVAL_PRIMING_MAX_RESULTS=7 must parse"
        );
        assert!(
            (cfg.priming_recurrence_weight - 0.05).abs() < 1e-6,
            "RETRIEVAL_PRIMING_RECURRENCE_WEIGHT=0.05 must parse; got {}",
            cfg.priming_recurrence_weight
        );
        assert_eq!(
            cfg.priming_freshness_slots, 2,
            "RETRIEVAL_PRIMING_FRESHNESS_SLOTS=2 must parse"
        );
        assert_eq!(
            cfg.priming_freshness_window_days, 60,
            "RETRIEVAL_PRIMING_FRESHNESS_WINDOW_DAYS=60 must parse"
        );
    }

    /// T12 Unit 3: When env vars are absent, `from_env()` uses the documented defaults
    /// for all five priming-scoped fields (no silent env fallback to zero/wrong values).
    #[test]
    fn priming_config_env_absent_falls_back_to_defaults() {
        let _g1 = EnvVarGuard::remove("RETRIEVAL_PRIMING_RELEVANCE_THRESHOLD");
        let _g2 = EnvVarGuard::remove("RETRIEVAL_PRIMING_MAX_RESULTS");
        let _g3 = EnvVarGuard::remove("RETRIEVAL_PRIMING_RECURRENCE_WEIGHT");
        let _g4 = EnvVarGuard::remove("RETRIEVAL_PRIMING_FRESHNESS_SLOTS");
        let _g5 = EnvVarGuard::remove("RETRIEVAL_PRIMING_FRESHNESS_WINDOW_DAYS");

        let cfg = RetrievalConfig::from_env();
        let default = RetrievalConfig::default();

        assert!(
            (cfg.priming_relevance_threshold - default.priming_relevance_threshold).abs() < 1e-6,
            "absent env must use default priming_relevance_threshold"
        );
        assert_eq!(
            cfg.priming_max_results, default.priming_max_results,
            "absent env must use default priming_max_results"
        );
        assert_eq!(
            cfg.priming_freshness_slots, default.priming_freshness_slots,
            "absent env must use default priming_freshness_slots"
        );
        assert_eq!(
            cfg.priming_freshness_window_days, default.priming_freshness_window_days,
            "absent env must use default priming_freshness_window_days"
        );
    }

    /// T12 Unit 3: `RetrievalSnapshot::with_skill_age_days` round-trip test.
    ///
    /// After calling the builder with a populated map, the snapshot must expose the
    /// same map via `skill_age_days`. An empty map (default from `new`) must not
    /// mark any skill as fresh — zero behavior change for existing tests and snapshots.
    #[test]
    fn retrieval_snapshot_with_skill_age_days_round_trip() {
        let snapshot = RetrievalSnapshot::new(vec![], 1);
        assert!(
            snapshot.skill_age_days.is_empty(),
            "default skill_age_days must be empty (no-freshness-data state)"
        );

        let mut age_map = std::collections::HashMap::new();
        age_map.insert("skill-a".to_owned(), 5_u32);
        age_map.insert("skill-b".to_owned(), 45_u32);

        let snapshot_with_ages = snapshot.with_skill_age_days(age_map.clone());
        assert_eq!(
            snapshot_with_ages.skill_age_days, age_map,
            "with_skill_age_days must attach the map to the snapshot"
        );
        assert_eq!(
            snapshot_with_ages.skill_age_days.get("skill-a").copied(),
            Some(5),
            "skill-a must have age 5"
        );
        assert_eq!(
            snapshot_with_ages.skill_age_days.get("skill-b").copied(),
            Some(45),
            "skill-b must have age 45"
        );
        assert_eq!(
            snapshot_with_ages.skill_age_days.get("skill-x"),
            None,
            "unknown skill must return None"
        );
    }

    /// T12 Unit 3 floor divergence: a skill that scores ~0.35 (above the 0.30
    /// priming floor but below the 0.48 Task floor) is:
    ///   - DROPPED by Task (0.35 < 0.48 → no_match)
    ///   - SURFACED by Priming (0.35 ≥ 0.30 → appears in result)
    ///
    /// Proves that the priming effective config applies the lower floor to Priming
    /// retrieval while the Task path keeps the calibrated 0.48 floor unchanged.
    ///
    /// Setup: single skill, embedding [1.0,0,0,0]. Query = [1.0,0,0,0].
    /// cosine = 1.0.  eq3 = α * 1.0 = 0.45 (default α=0.45, β=0, γ=0).
    /// 0.45 > 0.30 (priming floor) but 0.45 < 0.48 (task floor).
    #[tokio::test]
    async fn priming_lower_floor_surfaces_skill_below_task_threshold() {
        use domain::{DomainId, LifecycleStatus, ScopeType, Skill, SkillStatus};

        let skill = Skill {
            id: DomainId::new_unchecked("mid-score-skill"),
            name: "mid-score skill".to_owned(),
            description: "scores between priming and task floors".to_owned(),
            scope: ScopeType::Global,
            status: SkillStatus::Ready,
            lifecycle: LifecycleStatus::Active,
            tags: vec![],
            subunit_ids: vec![],
            community_id: None,
        };

        let snapshot = RetrievalSnapshot::new(
            vec![SeededSkill {
                skill,
                scope_id: "global".to_owned(),
                source_paths: vec![],
                // ConstantEmbeddingService returns [1,0,0,0]; cosine with [1,0,0,0] = 1.0
                embedding: vec![1.0, 0.0, 0.0, 0.0],
                subunits: vec![],
                subunit_embeddings: vec![],
                prior: 0.0,
                community_boost: 0.0,
                e_task_embedding: vec![],
                e_needs_embedding: vec![],
                e_negative_embedding: vec![],
            }],
            1,
        );

        // Config: Task floor = 0.48 (calibrated default), Priming floor = 0.30.
        // eq3 with α=0.45, cosine=1.0, β=0, γ=0 → score = 0.45.
        // 0.45 < 0.48 → Task drops it. 0.45 ≥ 0.30 → Priming surfaces it.
        let config = RetrievalConfig {
            scope_id: "global".to_owned(),
            scope_type: domain::ScopeType::Global,
            candidate_limit: 10,
            max_results: 3,
            relevance_threshold: 0.48,         // Task floor (calibrated)
            priming_relevance_threshold: 0.30, // Priming floor
            priming_max_results: 5,
            dense_views_enabled: false, // single-view scoring keeps eq3 simple
            backend: RetrievalBackend::SnapshotDense,
            ..RetrievalConfig::default()
        };

        let orchestrator =
            RetrievalOrchestrator::new(Arc::new(ConstantEmbeddingService), snapshot, config);

        // Task: 0.45 < 0.48 floor → no results.
        let task_outcome = orchestrator
            .retrieve("probe", None, RetrievalIntent::Task)
            .await;

        // Priming: 0.45 ≥ 0.30 floor → skill surfaced.
        let priming_outcome = orchestrator
            .retrieve("probe", None, RetrievalIntent::Priming)
            .await;

        assert!(
            task_outcome.skills.is_empty(),
            "Task must drop the skill (eq3=0.45 < Task floor 0.48); got {} skills",
            task_outcome.skills.len()
        );
        assert_eq!(
            priming_outcome.skills.len(),
            1,
            "Priming must surface the skill (eq3=0.45 ≥ Priming floor 0.30); got {} skills",
            priming_outcome.skills.len()
        );
        assert_eq!(
            priming_outcome.skills[0].scored_skill.skill.id.as_str(),
            "mid-score-skill",
            "the surfaced skill must be mid-score-skill"
        );
    }

    /// T12 Unit 1 seam guard: calling `retrieve(.., RetrievalIntent::Priming)` on a
    /// snapshot-dense orchestrator returns an outcome equal to `retrieve(.., RetrievalIntent::Task)`
    /// for the same snapshot and prompt — the Priming variant runs the identical code path
    /// in Unit 1 (pure seam; behavioral differentiation is added in later T12 units).
    ///
    /// Compares `skills` ids + scores and `reason_codes` because those are the
    /// semantically meaningful fields; latency_ms is excluded (wall-clock noise).
    ///
    /// NOTE (T12 Unit 3): This test uses `ConstantEmbeddingService` + `versioned_snapshot`
    /// where all skills have embedding [1,0,0,0]. With default floors (Task=0.48,
    /// Priming=0.30) and eq3=0.45 (α*cosine=0.45*1.0), ALL skills score BELOW Task 0.48
    /// and ABOVE Priming 0.30. The test scenario uses `versioned_snapshot(0)` = no skills,
    /// so both Task and Priming return empty results — the equality holds.
    ///
    /// The "no-divergence guard" for the case where all candidates clear BOTH floors
    /// is covered by `priming_single_segment_prompt_equals_task_outcome` above, which
    /// uses a lower test-specific floor. See also `priming_lower_floor_surfaces_skill_below_task_threshold`.
    #[tokio::test]
    async fn priming_intent_produces_identical_outcome_to_task_intent() {
        // Use an empty snapshot (version=0, no skills) so both Task and Priming
        // return empty results — the equality holds regardless of floor differences.
        // The `versioned_snapshot(3)` scenario diverges in Unit 3 (eq3=0.45 falls
        // between the Task floor 0.48 and the Priming floor 0.30); that divergence
        // is the intended Unit 3 behavior and is tested separately in
        // `priming_lower_floor_surfaces_skill_below_task_threshold`.
        let orchestrator = RetrievalOrchestrator::new(
            Arc::new(ConstantEmbeddingService),
            versioned_snapshot(0),
            RetrievalConfig::default(),
        );

        let task_outcome = orchestrator
            .retrieve("probe", None, RetrievalIntent::Task)
            .await;
        let priming_outcome = orchestrator
            .retrieve("probe", None, RetrievalIntent::Priming)
            .await;

        // Both return empty skills and no reason_codes (empty snapshot = no_match-style outcome).
        let task_ids: Vec<_> = task_outcome
            .skills
            .iter()
            .map(|s| {
                (
                    s.scored_skill.skill.id.as_str().to_owned(),
                    s.scored_skill.score.to_bits(),
                )
            })
            .collect();
        let priming_ids: Vec<_> = priming_outcome
            .skills
            .iter()
            .map(|s| {
                (
                    s.scored_skill.skill.id.as_str().to_owned(),
                    s.scored_skill.score.to_bits(),
                )
            })
            .collect();
        assert_eq!(
            task_ids, priming_ids,
            "T12 Unit 1 seam: empty snapshot → Priming and Task both return empty skills"
        );
        assert_eq!(
            task_outcome.reason_codes, priming_outcome.reason_codes,
            "T12 Unit 1 seam: empty snapshot → identical reason_codes"
        );
    }
}
