pub mod bm25;
pub mod circuit_breaker;
pub mod cosine_rank;
pub mod dense_views;
pub mod dual_scope;
pub mod fusion;
pub mod graph_search;
pub mod hybrid;
pub mod orchestrator;
pub mod scope_resolution;
pub mod scoring;
pub mod sparse;
pub(crate) mod text;

pub use bm25::{Bm25Index, SkillLexicalFields, skill_lexical_document};
pub use circuit_breaker::{CircuitBreaker, CircuitState};
pub use cosine_rank::{CosineHit, rank_by_cosine};
pub use dense_views::{
    SkillDenseViewFields, build_e_needs, build_e_negative, build_e_task, fuse_dense_views,
};
pub use dual_scope::{ScopedSearchFailure, ScopedSearchResult, search_scopes_concurrently};
pub use fusion::{FusedCandidate, ScopeRanking, mmr_select, weighted_reciprocal_rank_fusion};
pub use graph_search::{GraphHit, SubunitProjection, search_graph};
pub use hybrid::{HybridCandidate, HybridCandidateSource, HybridQueryError};
pub use orchestrator::{
    BoolFlag, CommunityBoostMode, DenseViewsMetadata, RescueCue, RetrievalBackend, RetrievalConfig,
    RetrievalIntent, RetrievalOrchestrator, RetrievalOutcome, RetrievalSnapshot, RetrievedSkill,
    RetrievedSubunit, SeededSkill, SkillRetriever,
};
pub use scope_resolution::{DualScopeResolver, ScopeResolutionOutcome};
pub use scoring::{ScoreComponents, ScoringWeights, UsagePriorInputs, score_eq3, usage_prior};
pub use sparse::{build_skill_sparse_vectors, query_sparse_vector, term_to_sparse_index};
